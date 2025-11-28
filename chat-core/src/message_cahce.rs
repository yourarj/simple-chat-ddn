use std::{
  collections::HashMap,
  hash::{DefaultHasher, Hash, Hasher},
  sync::Arc,
};

use bincode::Encode;
use bytes::Bytes;
use tokio::sync::RwLock;

use crate::protocol::BINCODE_STANDADRD_CONFIG;

pub struct MessageCache {
  cache: Arc<RwLock<HashMap<u64, Bytes>>>,
  capacity: usize,
}

impl MessageCache {
  pub fn new(capacity: usize) -> Self {
    Self {
      cache: Arc::new(RwLock::new(HashMap::new())),
      capacity,
    }
  }

  pub async fn get(&self, key: u64) -> Option<Bytes> {
    let cache = self.cache.read().await;
    cache.get(&key).cloned()
  }

  pub async fn put(&self, key: u64, value: Bytes) {
    let mut cache = self.cache.write().await;
    if cache.len() >= self.capacity
      && let Some(key) = cache.keys().next().cloned()
    {
      cache.remove(&key);
    }
    cache.insert(key, value);
  }

  pub fn hash_message<T: Encode>(message: &T) -> Result<u64, bincode::error::EncodeError> {
    let encoded = bincode::encode_to_vec(message, BINCODE_STANDADRD_CONFIG)?;
    let mut hasher = DefaultHasher::new();
    encoded.hash(&mut hasher);
    Ok(hasher.finish())
  }
}

impl Clone for MessageCache {
  fn clone(&self) -> Self {
    Self {
      cache: Arc::clone(&self.cache),
      capacity: self.capacity,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn test_message_cache_new() {
    let cache = MessageCache::new(10);
    assert_eq!(cache.capacity, 10);
  }

  #[tokio::test]
  async fn test_message_cache_get_put() {
    let cache = MessageCache::new(2);
    let test_message = "test message".to_string();
    
    let hash = MessageCache::hash_message(&test_message).unwrap();
    
    cache.put(hash, Bytes::from(test_message.clone())).await;
    
    let result = cache.get(hash).await;
    assert!(result.is_some());
    assert_eq!(result.unwrap(), Bytes::from(test_message));
  }

  #[tokio::test]
  async fn test_message_cache_capacity_limits() {
    let cache = MessageCache::new(2);
    
    let message1 = "message1".to_string();
    let message2 = "message2".to_string();
    let message3 = "message3".to_string();
    
    let hash1 = MessageCache::hash_message(&message1).unwrap();
    let hash2 = MessageCache::hash_message(&message2).unwrap();
    let hash3 = MessageCache::hash_message(&message3).unwrap();
    
    cache.put(hash1, Bytes::from(message1.clone())).await;
    cache.put(hash2, Bytes::from(message2.clone())).await;
    
    assert!(cache.get(hash1).await.is_some());
    assert!(cache.get(hash2).await.is_some());
    
    cache.put(hash3, Bytes::from(message3.clone())).await;
    
    let result1 = cache.get(hash1).await;
    let result2 = cache.get(hash2).await;
    let result3 = cache.get(hash3).await;
    
    assert!(result3.is_some());
    assert_eq!(result3.unwrap(), Bytes::from(message3));
    
    assert!(result1.is_none() || result2.is_none());
  }

  #[test]
  fn test_hash_message() {
    let message1 = "test message".to_string();
    let message2 = "different message".to_string();
    
    let hash1 = MessageCache::hash_message(&message1).unwrap();
    let hash2 = MessageCache::hash_message(&message2).unwrap();
    
    assert_ne!(hash1, hash2);
  }

  #[test]
  fn test_hash_message_same_content() {
    let message1 = "test message".to_string();
    let message2 = "test message".to_string();
    
    let hash1 = MessageCache::hash_message(&message1).unwrap();
    let hash2 = MessageCache::hash_message(&message2).unwrap();
    
    assert_eq!(hash1, hash2);
  }
}
