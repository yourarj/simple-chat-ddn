use chat_core::{
  error::{ApplicationError, Result},
  message_cache::MessageCache,
  protocol::SharedServerMessage,
};
use std::sync::Arc;
use tokio::{
  net::TcpListener,
  sync::{Semaphore, broadcast},
};
use tracing::{debug, info, warn};

use crate::{broadcast_pool::BroadcastPool, client_handler::handle_client};

pub type ShutdownSignal = tokio::sync::oneshot::Receiver<()>;

pub struct ChatServer {
  broadcast_pool: BroadcastPool,
  cache: MessageCache,
  max_connections: usize,
}

impl ChatServer {
  pub fn new(max_connections: usize, cache_capacity: usize, broadcast_capacity: usize) -> Self {
    let (tx, _rx) = broadcast::channel::<SharedServerMessage>(broadcast_capacity);
    let broadcaster = Arc::new(tx);
    let broadcast_pool = BroadcastPool::new(broadcaster);
    let cache = MessageCache::new(cache_capacity);

    Self {
      broadcast_pool,
      cache,
      max_connections,
    }
  }

  pub async fn run(&self, host: &str, port: u16, mut shutdown_rx: ShutdownSignal) -> Result<()> {
    let addr = format!("{}:{}", host, port);
    let listener = match TcpListener::bind(&addr).await {
      Ok(l) => l,
      Err(e) => {
        return Err(ApplicationError::Io(e));
      }
    };

    info!("Chat server listening on {}", addr);
    info!(
      "Configuration: max_connections={}, cache_capacity={}",
      self.max_connections,
      self.cache.capacity()
    );

    let limiter = Arc::new(Semaphore::new(self.max_connections));
    let mut connection_count: usize = 0;

    loop {
      tokio::select! {
          _ = &mut shutdown_rx => {
              info!("Shutdown signal received");
              self.shutdown().await?;
              info!("Total connections served: {}", connection_count);
              return Ok(());
          }

          accept_result = listener.accept() => {
              match accept_result {
                  Ok((stream, addr)) => {
                      // Try non-blocking acquire for immediate rejection
                      match limiter.clone().try_acquire_owned() {
                          Ok(permit) => {
                              debug!("Client connected from: {} (active: {})",
                                     addr, self.max_connections - limiter.available_permits());

                              connection_count = connection_count.saturating_add(1);

                              let pool = self.broadcast_pool.clone();
                              let cache = self.cache.clone();

                              tokio::spawn(async move {
                                  match handle_client(stream, pool, cache).await {
                                      Ok(()) => {
                                          debug!("Client {} disconnected cleanly", addr);
                                      }
                                      Err(e) => {
                                          debug!("Client {} error: {}", addr, e);
                                      }
                                  }
                                  drop(permit); // Release connection slot
                              });
                          }
                          Err(_) => {
                              warn!(
                                  "Max connections ({}) reached, rejecting connection from {}",
                                  self.max_connections, addr
                              );
                              // Stream drops here, closing connection
                          }
                      }
                  }
                  Err(e) => {
                      warn!("Failed to accept connection: {}", e);
                      // Continue accepting other connections
                  }
              }
          }
      }
    }
  }

  pub async fn shutdown(&self) -> Result<()> {
    info!("Shutting down chat server...");
    self.broadcast_pool.shutdown().await?;
    info!("Broadcast pool shutdown complete");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_server_creation() {
    let server = ChatServer::new(1000, 10_000, 50_000);
    assert_eq!(server.max_connections, 1000);
    assert_eq!(server.cache.capacity(), 10_000);
  }

  #[tokio::test]
  async fn test_server_shutdown() {
    let server = ChatServer::new(100, 1000, 10_000);
    let result = server.shutdown().await;
    assert!(result.is_ok());
  }
}
