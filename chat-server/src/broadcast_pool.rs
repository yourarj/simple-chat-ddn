use std::{collections::HashMap, sync::Arc};

use chat_core::{
  error::ApplicationError, message_cahce::MessageCache, protocol::SharedServerMessage,
};
use dashmap::DashMap;
use tokio::{net::tcp::OwnedWriteHalf, sync::broadcast::Sender, task::JoinHandle};
use tracing::warn;

pub struct BroadcastPool {
  broadcaster: Arc<Sender<SharedServerMessage>>,
  dispatchers: Arc<DashMap<String, JoinHandle<Result<(), ApplicationError>>>>,
}

impl BroadcastPool {
  pub fn new(broadcaster: Arc<Sender<SharedServerMessage>>) -> Self {
    Self {
      broadcaster,
      dispatchers: Arc::new(DashMap::new()),
    }
  }

  pub fn create_dispatcher(&self, username: String, writer: OwnedWriteHalf, cache: MessageCache) {
    let receiver = self.broadcaster.subscribe();
    let dispatcher =
      crate::broadcast::spawn_broadcast_dispatcher(receiver, username.clone(), writer, cache);
    self.dispatchers.insert(username, dispatcher);
  }

  pub fn has(&self, username: &str) -> bool {
    self.dispatchers.contains_key(username)
  }

  pub fn destroy_dispatcher(&self, username: &str) {
    if let Some((_, dispatcher)) = self.dispatchers.remove(username) {
      dispatcher.abort();
    } else {
      warn!("dispatcher not found for user {username}");
    }
  }

  pub async fn broadcast_message(
    &self,
    message: SharedServerMessage,
  ) -> Result<(), ApplicationError> {
    if self.broadcaster.send(message).is_err() {
      warn!("no recievers to recive broadcst");
    }
    Ok(())
  }

  pub async fn shutdown(&self) -> Result<(), ApplicationError> {
    drop(self.broadcaster.clone());

    let mut dispatchers = HashMap::new();
    for entry in self.dispatchers.iter() {
      if let Some((username, dispatcher)) = self.dispatchers.remove(entry.key()) {
        dispatchers.insert(username, dispatcher);
      }
    }

    for (_, dispatcher) in dispatchers {
      dispatcher.abort();
    }
    Ok(())
  }
}

impl Clone for BroadcastPool {
  fn clone(&self) -> Self {
    Self {
      broadcaster: Arc::clone(&self.broadcaster),
      dispatchers: Arc::clone(&self.dispatchers),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::sync::broadcast;

  #[test]
  fn test_broadcast_pool_new() {
    let (tx, _) = broadcast::channel(10);
    let broadcaster = Arc::new(tx);
    let pool = BroadcastPool::new(broadcaster.clone());

    assert!(Arc::ptr_eq(&pool.broadcaster, &broadcaster));
  }

  #[test]
  fn test_broadcast_pool_has() {
    let (tx, _) = broadcast::channel(10);
    let pool = BroadcastPool::new(Arc::new(tx));

    assert!(!pool.has("testuser"));
  }

  #[test]
  fn test_broadcast_pool_destroy_dispatcher() {
    let (tx, _) = broadcast::channel(10);
    let pool = BroadcastPool::new(Arc::new(tx));

    pool.destroy_dispatcher("nonexistent");
  }

  #[tokio::test]
  async fn test_broadcast_pool_shutdown() {
    let (tx, _) = broadcast::channel(10);
    let pool = BroadcastPool::new(Arc::new(tx));

    let result = pool.shutdown().await;
    assert!(result.is_ok());
  }
}
