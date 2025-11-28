use anyhow::Result;
use chat_core::message_cahce::MessageCache;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{
  broadcast::{self},
  oneshot,
};
use tracing::{error, info};

use crate::broadcast_pool::BroadcastPool;

pub struct ChatServer {
  max_connections: usize,
  broadcast_pool: BroadcastPool,
  cache: MessageCache,
}

pub type ShutdownSignal = oneshot::Receiver<()>;

impl ChatServer {
  pub fn new(max_connections: usize) -> Self {
    let (tx, _) = broadcast::channel(10_000);
    Self {
      broadcast_pool: BroadcastPool::new(Arc::new(tx)),
      cache: MessageCache::new(10_0000),
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
              let permit = connection_limiter.clone().acquire_owned().await?;

              let pool = self.broadcast_pool.clone();
              let cache = self.cache.clone();
              tokio::spawn(async move {
                if let Err(e) = crate::client_handler::handle_client(stream, pool, cache).await {
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
    self.broadcast_pool.shutdown().await?;

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    info!("Server shutdown complete");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use chat_core::{error::ApplicationError, protocol::ServerMessage};

  #[tokio::test]
  async fn test_server_creation() {
    let server = ChatServer::new(100);
    assert_eq!(server.max_connections, 100);
  }

  #[tokio::test]
  async fn test_server_creation_with_zero_connections() {
    let server = ChatServer::new(0);
    assert_eq!(server.max_connections, 0);
  }

  #[tokio::test]
  async fn test_graceful_shutdown() {
    let server = ChatServer::new(10);

    let result = server.shutdown().await;
    assert!(result.is_ok());
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
