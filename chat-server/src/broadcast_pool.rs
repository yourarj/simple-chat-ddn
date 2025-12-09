use chat_core::{
  error::Result,
  message_cache::MessageCache,
  protocol::{SharedServerMessage, UserMessageCounter},
};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::{net::tcp::OwnedWriteHalf, task::JoinHandle};
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct BroadcastPool {
  broadcaster: Arc<tokio::sync::broadcast::Sender<SharedServerMessage>>,
  dispatchers: Arc<DashMap<String, JoinHandle<()>>>,
  counters: Arc<DashMap<String, Arc<UserMessageCounter>>>,
}

impl BroadcastPool {
  pub fn new(broadcaster: Arc<tokio::sync::broadcast::Sender<SharedServerMessage>>) -> Self {
    Self {
      broadcaster,
      dispatchers: Arc::new(DashMap::new()),
      counters: Arc::new(DashMap::new()),
    }
  }

  /// Create a new broadcast dispatcher for a user
  pub fn create_dispatcher(&self, username: String, writer: OwnedWriteHalf, cache: MessageCache) {
    // Create per-user message counter
    let counter = Arc::new(UserMessageCounter::new(username.clone()));
    self.counters.insert(username.clone(), Arc::clone(&counter));

    let receiver = self.broadcaster.subscribe();
    let dispatcher = crate::broadcast::spawn_broadcast_dispatcher(
      receiver,
      username.clone(),
      writer,
      cache,
      counter,
    );

    if let Some(old_handle) = self.dispatchers.insert(username.clone(), dispatcher) {
      warn!("Replaced existing dispatcher for user: {}", username);
      old_handle.abort();
    }

    debug!("Created dispatcher and counter for user: {}", username);
  }

  /// Check if user has an active dispatcher
  pub fn has(&self, username: &str) -> bool {
    self.dispatchers.contains_key(username)
  }

  /// Get user's message counter
  pub fn get_counter(&self, username: &str) -> Option<Arc<UserMessageCounter>> {
    self
      .counters
      .get(username)
      .map(|entry| Arc::clone(entry.value()))
  }

  /// Destroy a user's dispatcher and counter
  pub fn destroy_dispatcher(&self, username: &str) {
    match self.dispatchers.remove(username) {
      Some((_, dispatcher)) => {
        dispatcher.abort();
        debug!("Dispatcher destroyed for user: {}", username);
      }
      None => {
        debug!("No dispatcher found for user: {}", username);
      }
    }

    // Remove counter
    if let Some((_, counter)) = self.counters.remove(username) {
      info!(
        "User '{}' sent {} total messages",
        username,
        counter.current_count()
      );
    }
  }

  /// Broadcast message to all connected users
  pub async fn broadcast_message(&self, message: SharedServerMessage) -> Result<()> {
    match self.broadcaster.send(message) {
      Ok(receiver_count) => {
        debug!("Message broadcast to {} receivers", receiver_count);
        Ok(())
      }
      Err(_) => {
        debug!("No active receivers for broadcast");
        Ok(())
      }
    }
  }

  /// Shutdown all dispatchers
  pub async fn shutdown(&self) -> Result<()> {
    let usernames: Vec<String> = self
      .dispatchers
      .iter()
      .map(|entry| entry.key().clone())
      .collect();

    debug!("Shutting down {} dispatchers", usernames.len());

    for username in usernames {
      self.destroy_dispatcher(&username);
    }

    debug!("All dispatchers shut down");
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_counter_storage() {
    let (tx, _rx) = tokio::sync::broadcast::channel(100);
    let pool = BroadcastPool::new(Arc::new(tx));

    let counter = Arc::new(UserMessageCounter::new("alice".to_string()));
    pool.counters.insert("alice".to_string(), counter);

    assert!(pool.get_counter("alice").is_some());
    assert!(pool.get_counter("bob").is_none());
  }

  #[test]
  fn test_has_user() {
    let (tx, _rx) = tokio::sync::broadcast::channel(100);
    let pool = BroadcastPool::new(Arc::new(tx));
    assert!(!pool.has("alice"));
  }
}
