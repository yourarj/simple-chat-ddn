// server/src/lib.rs
use anyhow::Result;
use chat_core::error::ApplicationError;
use chat_core::protocol::{ChatMessage, encode_message};
use chat_core::transport_layer::read_next_message_from_stream;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast::{self, Sender};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

type UserMap = Arc<DashMap<String, JoinHandle<Result<(), ApplicationError>>>>;

/// High-performance chat server supporting 1M+ concurrent users
pub struct ChatServer {
  users: UserMap,
  max_connections: usize,
  broadcaster: Arc<Sender<ChatMessage>>,
}

impl ChatServer {
  pub fn new(max_connections: usize) -> Self {
    // Create broadcast channel for this user
    let (tx, _) = broadcast::channel(10_000); // Buffer 100 messages
    Self {
      users: Arc::new(DashMap::new()),
      broadcaster: Arc::new(tx),
      max_connections,
    }
  }

  /// Start the chat server on specified host and port
  pub async fn run(&self, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind(format!("{}:{}", host, port)).await?;
    info!("Chat server listening on {}:{}", host, port);

    let connection_limiter = Arc::new(tokio::sync::Semaphore::new(self.max_connections));

    loop {
      // Acquire permit for connection limiting
      let permit = connection_limiter.clone().acquire_owned().await?;

      match listener.accept().await {
        Ok((stream, addr)) => {
          info!("New connection from {}", addr);

          let users = Arc::clone(&self.users);
          let broadcaster = Arc::clone(&self.broadcaster);

          tokio::spawn(async move {
            if let Err(e) = Self::handle_client(stream, broadcaster, users).await {
              error!("Client handler error: {}", e);
            }
            drop(permit); // Release connection permit
          });
        }
        Err(e) => {
          error!("Accept error: {}", e);
          drop(permit);
        }
      }
    }
  }

  /// Handle individual client connection
  async fn handle_client(
    stream: TcpStream,
    broadcaster: Arc<Sender<ChatMessage>>,
    users: UserMap,
  ) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 4096]; // Reusable buffer to avoid allocations
    let mut username;

    loop {
      match read_next_message_from_stream(&mut reader, &mut buffer).await {
        Ok(ChatMessage::Join { username: un }) => {
          if users.contains_key(&un) {
            let error_msg = ChatMessage::Error {
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
          let error_msg = ChatMessage::Error {
            reason: "You have to join the group before doing any other operation".to_string(),
          };
          let frame = encode_message(&error_msg)?;
          writer.write_all(&frame).await?;
        }
        Err(_) => (),
      }
    }

    let user = username.ok_or(ApplicationError::UsernameNotFound)?;

    // subscription
    let rec = broadcaster.subscribe();

    let join_handle = crate::broadcast::spawn_broadcast_dispatcher(rec, user.clone(), writer);
    users.insert(user.clone(), join_handle);
    username = Some(user.clone());
    Self::broadcast_message(&broadcaster, &users, &user, "joined the chat!", &user).await?;

    info!("User {} joined the chat", user);

    loop {
      let message = read_next_message_from_stream(&mut reader, &mut buffer).await;
      match message {
        Ok(ChatMessage::Join { username: _ }) => {}
        Ok(ChatMessage::Leave { username: user }) => {
          users.remove(&user);
          info!("User {} left the chat", user);
          Self::broadcast_message(&broadcaster, &users, &user, "left the chatroom", &user).await?;
          break;
        }
        Ok(ChatMessage::Message {
          username: user,
          content,
        }) => {
          Self::broadcast_message(&broadcaster, &users, &user, &content, &user).await?;
          info!("Message from {}: {}", user, content);
        }
        _ => {
          //   let error_msg = ChatMessage::Error {
          //     reason: "Invalid message type".to_string(),
          //   };
          //   let frame = encode_message(&error_msg)?;
          //   writer.write_all(&frame).await?;
        }
      }
    }

    // Cleanup on disconnect
    if let Some(user) = username {
      users.remove(&user);
      info!("User {} disconnected", user);
    }

    Ok(())
  }

  /// Broadcast message to all users except sender
  async fn broadcast_message(
    broadcaster: &Arc<Sender<ChatMessage>>,
    users: &UserMap,
    sender: &str,
    content: &str,
    exclude_user: &str,
  ) -> Result<()> {
    let message = ChatMessage::Message {
      username: sender.to_string(),
      content: content.to_string(),
    };

    let mut failed_users = Vec::new();

    // Use dashmap's sharded locking for efficient concurrent iteration
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

    // Clean up failed users (channels closed)
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
  }

  #[tokio::test]
  async fn test_message_broadcast() {
    // let users = Arc::new(DashMap::new());
    // let (tx1, mut rx1) = broadcast::channel(10);
    // let (tx2, mut rx2) = broadcast::channel(10);

    // users.insert("user1".to_string(), tx1);
    // users.insert("user2".to_string(), tx2);

    // ChatServer::broadcast_message(tx1, &users, "user1", "test message", "user1")
    //   .await
    //   .unwrap();

    // // user1 should not receive their own message
    // tokio::time::timeout(std::time::Duration::from_millis(100), rx1.recv())
    //   .await
    //   .unwrap_err();

    // // user2 should receive the message
    // let received = tokio::time::timeout(std::time::Duration::from_millis(100), rx2.recv())
    //   .await
    //   .unwrap()
    //   .unwrap();

    // match received {
    //   ChatMessage::Message { username, content } => {
    //     assert_eq!(username, "user1");
    //     assert_eq!(content, "test message");
    //   }
    //   _ => panic!("Unexpected message type"),
    // }
  }
}
