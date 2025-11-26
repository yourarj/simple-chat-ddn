use anyhow::Result;
use chat_core::error::ApplicationError;
use chat_core::protocol::{ClientMessage, ServerMessage, encode_message};
use chat_core::transport_layer::read_message_from_stream;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{
  broadcast::{self, Sender},
  oneshot,
};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

type UserMap = Arc<DashMap<String, JoinHandle<Result<(), ApplicationError>>>>;

pub struct ChatServer {
  users: UserMap,
  max_connections: usize,
  broadcaster: Arc<Sender<ServerMessage>>,
}

pub type ShutdownSignal = oneshot::Receiver<()>;

impl ChatServer {
  pub fn new(max_connections: usize) -> Self {
    let (tx, _) = broadcast::channel(10_000);
    Self {
      users: Arc::new(DashMap::new()),
      broadcaster: Arc::new(tx),
      max_connections,
    }
  }

  pub async fn run(&self, host: &str, port: u16, mut shutdown_rx: ShutdownSignal) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await?;
    info!("Chat server listening on {}:{}", host, port);

    let connection_limiter = Arc::new(tokio::sync::Semaphore::new(self.max_connections));

    loop {
      tokio::select! {
        _ = &mut shutdown_rx => {
          info!("Shutdown signal received, starting graceful shutdown...");
          self.shutdown().await?;
          return Ok(());
        }
        accept_result = listener.accept() => {
          match accept_result {
            Ok((stream, addr)) => {
              info!("New connection from {}", addr);

              let users = Arc::clone(&self.users);
              let broadcaster = Arc::clone(&self.broadcaster);
              let permit = connection_limiter.clone().acquire_owned().await?;

              tokio::spawn(async move {
                if let Err(e) = Self::handle_client(stream, broadcaster, users).await {
                  error!("Client handler error: {}", e);
                }
                drop(permit);
              });
            }
            Err(e) => {
              error!("Accept error: {}", e);
              tracing::debug!("Accept failed, releasing permit");


            }
          }
        }
      }
    }
  }

  async fn shutdown(&self) -> Result<()> {
    info!("Starting graceful shutdown...");

    drop(Arc::clone(&self.broadcaster));

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let mut user_tasks = Vec::new();
    for entry in self.users.iter() {
      if let Some((_, join_handle)) = self.users.remove(entry.key()) {
        user_tasks.push(join_handle);
      }
    }

    for join_handle in user_tasks {
      join_handle.abort();
      let _ = join_handle.await;
    }

    info!("Server shutdown complete");
    Ok(())
  }

  async fn handle_client(
    stream: TcpStream,
    broadcaster: Arc<Sender<ServerMessage>>,
    users: UserMap,
  ) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 4096];
    let mut username;

    loop {
      match read_message_from_stream(&mut reader, &mut buffer).await {
        Ok(ClientMessage::Join { username: un }) => {
          if users.contains_key(&un) {
            let error_msg = ServerMessage::Error {
              reason: format!("Username `{}` has already been taken", &un),
            };
            let frame = encode_message(&error_msg)?;
            writer.write_all(&frame).await?;
            continue;
          }
          username = Some(un);
          break;
        }
        Ok(_) => {
          let error_msg = ServerMessage::Error {
            reason: "You have to join the group before doing any other operation".to_string(),
          };
          let frame = encode_message(&error_msg)?;
          writer.write_all(&frame).await?;
        }
        Err(_) => (),
      }
    }

    let user = username.ok_or(ApplicationError::UsernameNotFound)?;

    let rec = broadcaster.subscribe();

    let join_handle = crate::broadcast::spawn_broadcast_dispatcher(rec, user.clone(), writer);
    users.insert(user.clone(), join_handle);
    username = Some(user.clone());

    let join_notification = ServerMessage::UserJoined {
      username: user.clone(),
    };
    Self::broadcast_message(&broadcaster, &users, join_notification, &user).await?;

    info!("User {} joined the chat", user);

    loop {
      let message = read_message_from_stream(&mut reader, &mut buffer).await;
      match message {
        Ok(ClientMessage::Join { username: _ }) => {}
        Ok(ClientMessage::Leave { username: user }) => {
          if let Some((_, join_handle)) = users.remove(&user) {
            join_handle.abort();
          }
          info!("User {} left the chat", user);

          let leave_notification = ServerMessage::UserLeft {
            username: user.clone(),
          };
          Self::broadcast_message(&broadcaster, &users, leave_notification, &user).await?;
          break;
        }
        Ok(ClientMessage::Message {
          username: user,
          content,
        }) => {
          let message_notification = ServerMessage::Message {
            username: user.clone(),
            content: content.clone(),
          };
          Self::broadcast_message(&broadcaster, &users, message_notification, &user).await?;
          info!("Message from {}: {}", user, content);
        }
        _ => {}
      }
    }

    if let Some(user) = username {
      tracing::debug!("Cleaning up disconnected user: {}", user);
      if let Some((_, join_handle)) = users.remove(&user) {
        join_handle.abort();
      }
      info!("User {} disconnected", user);
    }

    Ok(())
  }

  async fn broadcast_message(
    broadcaster: &Arc<Sender<ServerMessage>>,
    users: &UserMap,
    message: ServerMessage,
    exclude_user: &str,
  ) -> Result<()> {
    let mut failed_users = Vec::new();

    for entry in users.iter() {
      let user = entry.key();
      if user == exclude_user {
        continue;
      }

      if let Err(e) = broadcaster.send(message.clone()) {
        warn!("Failed to send message to {}: {}", user, e);
        failed_users.push(user.clone());
      }
    }

    for user in failed_users {
      users.remove(&user);
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_server_creation() {
    let server = ChatServer::new(100);
    assert_eq!(server.users.len(), 0);
    assert_eq!(server.max_connections, 100);

    let _subscriber = server.broadcaster.subscribe();
  }

  #[tokio::test]
  async fn test_server_creation_with_zero_connections() {
    let server = ChatServer::new(0);
    assert_eq!(server.max_connections, 0);

    assert_eq!(server.users.len(), 0);
  }

  #[tokio::test]
  async fn test_graceful_shutdown() {
    let server = ChatServer::new(10);

    let result = server.shutdown().await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_user_map_operations() {
    let server = ChatServer::new(100);

    assert_eq!(server.users.len(), 0);

    let test_user = "testuser";
    let join_handle = tokio::spawn(async move { Ok::<(), ApplicationError>(()) });
    server.users.insert(test_user.to_string(), join_handle);

    assert_eq!(server.users.len(), 1);
    assert!(server.users.contains_key(test_user));

    if let Some((_, join_handle)) = server.users.remove(test_user) {
      join_handle.abort();
    }

    assert_eq!(server.users.len(), 0);
    assert!(!server.users.contains_key(test_user));
  }

  #[tokio::test]
  async fn test_username_collision_detection() {
    let server = ChatServer::new(100);

    let test_user = "testuser";

    let join_handle1 = tokio::spawn(async move { Ok::<(), ApplicationError>(()) });
    let join_handle2 = tokio::spawn(async move { Ok::<(), ApplicationError>(()) });

    server.users.insert(test_user.to_string(), join_handle1);
    assert!(server.users.contains_key(test_user));

    server.users.insert(test_user.to_string(), join_handle2);
    assert!(server.users.contains_key(test_user));
    assert_eq!(server.users.len(), 1);
  }

  #[tokio::test]
  async fn test_broadcast_message_empty_users() {
    let server = ChatServer::new(100);

    let test_message = ServerMessage::Message {
      username: "sender".to_string(),
      content: "Hello everyone!".to_string(),
    };

    let result =
      ChatServer::broadcast_message(&server.broadcaster, &server.users, test_message, "sender")
        .await;

    assert!(result.is_ok());
    assert_eq!(server.users.len(), 0);
  }

  #[test]
  fn test_application_error_variants() {
    let errors = vec![
      ApplicationError::ClientReadStreamClosed,
      ApplicationError::IncompleteLengthPrefix,
      ApplicationError::IncompletePyaload,
      ApplicationError::UsernameNotFound,
    ];

    for error in errors {
      let error_string = format!("{}", error);
      assert!(!error_string.is_empty(), "Error should have a description");

      let _debug_string = format!("{:?}", error);
    }
  }

  #[tokio::test]
  async fn test_multiple_server_instances() {
    let server1 = ChatServer::new(50);
    let server2 = ChatServer::new(100);

    assert_eq!(server1.max_connections, 50);
    assert_eq!(server2.max_connections, 100);
    assert_eq!(server1.users.len(), 0);
    assert_eq!(server2.users.len(), 0);

    let _subscriber1 = server1.broadcaster.subscribe();
    let _subscriber2 = server2.broadcaster.subscribe();

    let join_handle = tokio::spawn(async move { Ok::<(), ApplicationError>(()) });
    server1.users.insert("user1".to_string(), join_handle);

    assert_eq!(server1.users.len(), 1);
    assert_eq!(server2.users.len(), 0);
    assert!(server1.users.contains_key("user1"));
    assert!(!server2.users.contains_key("user1"));
  }

  #[test]
  fn test_connection_limiter_creation() {
    let server = ChatServer::new(10);

    let connection_limiter = tokio::sync::Semaphore::new(server.max_connections);
    assert_eq!(connection_limiter.available_permits(), 10);

    let permit1 = connection_limiter.try_acquire().unwrap();
    assert_eq!(connection_limiter.available_permits(), 9);

    let permit2 = connection_limiter.try_acquire().unwrap();
    assert_eq!(connection_limiter.available_permits(), 8);

    drop(permit1);
    drop(permit2);

    assert_eq!(connection_limiter.available_permits(), 10);
  }

  #[test]
  fn test_large_server_capacity() {
    let server = ChatServer::new(1_000_000);
    assert_eq!(server.max_connections, 1_000_000);

    assert_eq!(server.users.len(), 0);
    let _subscriber = server.broadcaster.subscribe();
  }

  #[test]
  fn test_server_message_username_extraction_comprehensive() {
    let messages_with_usernames = vec![
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "Hello".to_string(),
      },
      ServerMessage::UserJoined {
        username: "bob".to_string(),
      },
      ServerMessage::UserLeft {
        username: "charlie".to_string(),
      },
    ];

    let messages_without_usernames = vec![
      ServerMessage::Success {
        message: "Welcome!".to_string(),
      },
      ServerMessage::Error {
        reason: "Error occurred".to_string(),
      },
    ];

    for message in &messages_with_usernames {
      assert!(message.username().is_some(), "Message should have username");
      assert!(
        !message.username().unwrap().is_empty(),
        "Username should not be empty"
      );
    }

    for message in &messages_without_usernames {
      assert!(
        message.username().is_none(),
        "Message should not have username"
      );
    }
  }
}
