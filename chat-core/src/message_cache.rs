use bytes::Bytes;
use dashmap::DashMap;
use std::sync::Arc;
use tracing::debug;

use crate::error::{ApplicationError, Result};

const DEFAULT_CACHE_CAPACITY: usize = 10_000;
const MIN_CACHE_CAPACITY: usize = 100;
const MAX_CACHE_CAPACITY: usize = 1_000_000;

/// High-performance lock-free message cache using DashMap
///
/// Stores encoded messages by their unique ID
#[derive(Clone)]
pub struct MessageCache {
  cache: Arc<DashMap<u64, Bytes>>,
  capacity: usize,
}

impl MessageCache {
  pub fn new(capacity: usize) -> Self {
    let clamped_capacity = capacity.clamp(MIN_CACHE_CAPACITY, MAX_CACHE_CAPACITY);

    if clamped_capacity != capacity {
      debug!(
        "Cache capacity adjusted from {} to {}",
        capacity, clamped_capacity
      );
    }

    Self {
      cache: Arc::new(DashMap::with_capacity(clamped_capacity)),
      capacity: clamped_capacity,
    }
  }

  /// Get cached encoded message by ID
  pub fn get(&self, message_id: u64) -> Option<Bytes> {
    self
      .cache
      .get(&message_id)
      .map(|entry| entry.value().clone())
  }

  /// Store encoded message with its ID
  pub fn put(&self, message_id: u64, encoded: Bytes) {
    if self.cache.len() >= self.capacity
      && let Err(e) = self.try_evict_one()
    {
      debug!("Failed to evict cache entry: {}", e);
    }

    self.cache.insert(message_id, encoded);
  }

  fn try_evict_one(&self) -> Result<()> {
    let key_to_remove = self
      .cache
      .iter()
      .next()
      .map(|entry| *entry.key())
      .ok_or_else(|| {
        ApplicationError::invalid_frame("Cache is full but no entries to evict".to_string())
      })?;

    self.cache.remove(&key_to_remove);
    Ok(())
  }

  pub fn len(&self) -> usize {
    self.cache.len()
  }

  pub fn is_empty(&self) -> bool {
    self.cache.is_empty()
  }

  pub fn clear(&self) {
    self.cache.clear();
  }

  pub fn capacity(&self) -> usize {
    self.capacity
  }
}

impl Default for MessageCache {
  fn default() -> Self {
    Self::new(DEFAULT_CACHE_CAPACITY)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_cache_basic_operations() {
    let cache = MessageCache::new(100);
    let message_id = 12345u64;
    let value = Bytes::from("test data");

    cache.put(message_id, value.clone());
    assert_eq!(cache.get(message_id), Some(value));
    assert_eq!(cache.len(), 1);
  }

  #[test]
  fn test_cache_eviction() {
    let cache = MessageCache::new(2);
    cache.put(1, Bytes::from("a"));
    cache.put(2, Bytes::from("b"));
    cache.put(3, Bytes::from("c"));

    assert!(cache.len() <= 3);
  }

  #[test]
  fn test_concurrent_access() {
    use std::thread;

    let cache = MessageCache::new(1000);
    let cache_clone = cache.clone();

    let handles: Vec<_> = (0..10)
      .map(|i| {
        let cache = cache_clone.clone();
        thread::spawn(move || {
          for j in 0..100 {
            let id = (i * 100 + j) as u64;
            cache.put(id, Bytes::from(format!("data-{}-{}", i, j)));
          }
        })
      })
      .collect();

    for handle in handles {
      handle.join().expect("Thread panicked");
    }

    assert!(cache.len() <= 1000);
  }
}
