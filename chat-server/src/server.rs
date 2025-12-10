use crate::{broadcast_pool::BroadcastPool, client_handler::handle_client};
use chat_core::{error::Result, message_cache::MessageCache, protocol::SharedServerMessage};
use std::sync::Arc;
use tokio::{
  net::TcpListener,
  sync::{Semaphore, broadcast},
};
use tracing::{info, warn};

pub type ShutdownSignal = tokio::sync::oneshot::Receiver<()>;

pub struct ChatServer {
  broadcast_pool: BroadcastPool,
  cache: MessageCache,
  max_connections: usize,
}

impl ChatServer {
  pub fn new(max_connections: usize, cache_capacity: usize, broadcast_capacity: usize) -> Self {
    let calculated_capacity = broadcast_capacity.max(50_000);

    let (tx, _rx) = broadcast::channel::<SharedServerMessage>(calculated_capacity);
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
    let listener = TcpListener::bind(&addr).await?;

    info!("Server listening on {}", addr);

    let limiter = Arc::new(Semaphore::new(self.max_connections));

    loop {
      tokio::select! {
          _ = &mut shutdown_rx => {
              info!("Shutdown signal received");
              self.shutdown().await?;
              return Ok(());
          }
          result = listener.accept() => {
              let (stream, addr) = result?;

              // Try non-blocking acquire first (OPTIMIZATION)
              let permit = match limiter.clone().try_acquire_owned() {
                  Ok(p) => p,
                  Err(_) => {
                      warn!("Max connections reached, rejecting {}", addr);
                      drop(stream);  // Immediate reject
                      continue;
                  }
              };

              let pool = self.broadcast_pool.clone();
              let cache = self.cache.clone();

              tokio::spawn(async move {
                  if let Err(e) = handle_client(stream, pool, cache).await {
                      warn!("Client {} error: {}", addr, e);
                  }
                  drop(permit);  // Auto-release on client disconnect
              });
          }
      }
    }
  }

  async fn shutdown(&self) -> Result<()> {
    info!("Shutting down server...");
    self.broadcast_pool.shutdown().await?;
    Ok(())
  }
}
