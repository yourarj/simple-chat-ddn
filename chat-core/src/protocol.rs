use bincode::{Decode, Encode};
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::debug;

use crate::error::{ApplicationError, Result};
use crate::message_cache::MessageCache;

pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;
pub const MAX_USERNAME_LENGTH: usize = 30;
pub const LENGTH_PREFIX: usize = 4;

const BINCODE_STANDARD_CONFIG: bincode::config::Configuration = bincode::config::standard();

#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum ClientMessage {
  Join { username: String },
  Leave { username: String },
  Message { username: String, content: String },
}

#[derive(Debug, Clone, Encode, Decode, PartialEq, Eq)]
pub enum ServerMessage {
  Success { message: String },
  Error { reason: String },
  Message { username: String, content: String },
  UserJoined { username: String },
  UserLeft { username: String },
}

impl ServerMessage {
  pub fn success(message: String) -> Self {
    Self::Success { message }
  }

  pub fn error(reason: String) -> Self {
    Self::Error { reason }
  }

  pub fn message(username: String, content: String) -> Self {
    Self::Message { username, content }
  }

  pub fn user_joined(username: String) -> Self {
    Self::UserJoined { username }
  }

  pub fn user_left(username: String) -> Self {
    Self::UserLeft { username }
  }

  pub fn user_name_already_taken(username: String) -> Self {
    Self::Error {
      reason: format!("Username '{}' is already taken", username),
    }
  }

  pub fn username(&self) -> Option<&str> {
    match self {
      Self::Message { username, .. }
      | Self::UserJoined { username }
      | Self::UserLeft { username } => Some(username),
      _ => None,
    }
  }
}

/// Message ID based on username hash + sequence counter
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId {
  username_hash: u64,
  sequence: u64,
}

impl MessageId {
  /// Create a new message ID from username and sequence
  pub fn new(username: &str, sequence: u64) -> Self {
    let mut hasher = DefaultHasher::new();
    username.hash(&mut hasher);
    let username_hash = hasher.finish();

    Self {
      username_hash,
      sequence,
    }
  }

  /// Convert to u64 for cache key (XOR for good distribution)
  pub fn as_u64(&self) -> u64 {
    self.username_hash ^ self.sequence
  }

  pub fn username_hash(&self) -> u64 {
    self.username_hash
  }

  pub fn sequence(&self) -> u64 {
    self.sequence
  }
}

/// Per-user message counter
#[derive(Debug)]
pub struct UserMessageCounter {
  username: String,
  username_hash: u64,
  counter: AtomicU64,
}

impl UserMessageCounter {
  pub fn new(username: String) -> Self {
    let mut hasher = DefaultHasher::new();
    username.hash(&mut hasher);
    let username_hash = hasher.finish();

    Self {
      username,
      username_hash,
      counter: AtomicU64::new(0),
    }
  }

  /// Generate next message ID for this user
  pub fn next_id(&self) -> MessageId {
    let sequence = self.counter.fetch_add(1, Ordering::Relaxed);
    MessageId {
      username_hash: self.username_hash,
      sequence,
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn current_count(&self) -> u64 {
    self.counter.load(Ordering::Relaxed)
  }
}

/// Shared server message with user-scoped ID
#[derive(Debug, Clone)]
pub struct SharedServerMessage {
  inner: Arc<ServerMessage>,
  id: MessageId,
}

impl SharedServerMessage {
  /// Create message with ID from user's counter
  pub fn new_with_counter(message: ServerMessage, counter: &UserMessageCounter) -> Self {
    let id = counter.next_id();
    Self {
      inner: Arc::new(message),
      id,
    }
  }

  /// Create message without counter (for system messages)
  pub fn new_system(message: ServerMessage) -> Self {
    static SYSTEM_COUNTER: AtomicU64 = AtomicU64::new(0);
    let sequence = SYSTEM_COUNTER.fetch_add(1, Ordering::Relaxed);
    let id = MessageId::new("system", sequence);

    Self {
      inner: Arc::new(message),
      id,
    }
  }

  pub fn get(&self) -> &ServerMessage {
    &self.inner
  }

  pub fn id(&self) -> MessageId {
    self.id
  }

  pub fn cache_key(&self) -> u64 {
    self.id.as_u64()
  }

  pub fn username(&self) -> Option<&str> {
    self.inner.username()
  }
}

/// Encode message with optimized single allocation
pub fn encode_message<T: Encode>(message: &T) -> Result<Bytes> {
  let payload_vec = bincode::encode_to_vec(message, BINCODE_STANDARD_CONFIG)?;
  let payload_len = payload_vec.len();

  if payload_len > MAX_MESSAGE_SIZE {
    return Err(ApplicationError::message_too_large(
      payload_len,
      MAX_MESSAGE_SIZE,
    ));
  }

  let total_size = LENGTH_PREFIX
    .checked_add(payload_len)
    .ok_or_else(|| ApplicationError::invalid_frame("Frame size overflow".to_string()))?;

  let mut frame = BytesMut::with_capacity(total_size);
  frame.put_u32(payload_len as u32);
  frame.put_slice(&payload_vec);

  Ok(frame.freeze())
}

/// Encode message with cache - uses username-based message ID
pub fn encode_message_with_cache(
  message: &SharedServerMessage,
  cache: &MessageCache,
) -> Result<Bytes> {
  let cache_key = message.cache_key();

  // Try cache first
  if let Some(cached) = cache.get(cache_key) {
    debug!("Cache hit for message ID: {:?}", message.id());
    return Ok(cached);
  }

  // Encode once on cache miss
  let frame_bytes = encode_message(message.get())?;
  cache.put(cache_key, frame_bytes.clone());

  debug!(
    "Cache miss, encoded and cached message ID: {:?}",
    message.id()
  );
  Ok(frame_bytes)
}

pub fn decode_message<T: Decode<()>>(bytes: &[u8]) -> Result<T> {
  let (decoded, _len) = bincode::decode_from_slice(bytes, BINCODE_STANDARD_CONFIG)?;
  Ok(decoded)
}

pub fn read_frame(buffer: &mut impl Buf) -> Result<Option<Bytes>> {
  if buffer.remaining() < LENGTH_PREFIX {
    return Ok(None);
  }

  let mut length_bytes = [0u8; LENGTH_PREFIX];
  let peek_buf = buffer.chunk();

  if peek_buf.len() < LENGTH_PREFIX {
    return Ok(None);
  }

  length_bytes.copy_from_slice(&peek_buf[..LENGTH_PREFIX]);
  let payload_len = u32::from_be_bytes(length_bytes) as usize;

  if payload_len > MAX_MESSAGE_SIZE {
    return Err(ApplicationError::message_too_large(
      payload_len,
      MAX_MESSAGE_SIZE,
    ));
  }

  let total_frame_size = LENGTH_PREFIX
    .checked_add(payload_len)
    .ok_or_else(|| ApplicationError::invalid_frame("Frame size overflow".to_string()))?;

  if buffer.remaining() < total_frame_size {
    return Ok(None);
  }

  buffer.advance(LENGTH_PREFIX);
  let payload = buffer.copy_to_bytes(payload_len);
  Ok(Some(payload))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_message_id_deterministic() {
    let id1 = MessageId::new("alice", 5);
    let id2 = MessageId::new("alice", 5);
    assert_eq!(id1, id2);
    assert_eq!(id1.as_u64(), id2.as_u64());
  }

  #[test]
  fn test_message_id_different_users() {
    let id1 = MessageId::new("alice", 1);
    let id2 = MessageId::new("bob", 1);
    assert_ne!(id1, id2);
    assert_ne!(id1.as_u64(), id2.as_u64());
  }

  #[test]
  fn test_message_id_different_sequence() {
    let id1 = MessageId::new("alice", 1);
    let id2 = MessageId::new("alice", 2);
    assert_ne!(id1, id2);
    assert_ne!(id1.as_u64(), id2.as_u64());
  }

  #[test]
  fn test_user_counter() {
    let counter = UserMessageCounter::new("alice".to_string());
    let id1 = counter.next_id();
    let id2 = counter.next_id();
    let id3 = counter.next_id();

    assert_ne!(id1, id2);
    assert_ne!(id2, id3);
    assert_eq!(counter.current_count(), 3);
  }

  #[test]
  fn test_concurrent_counter() {
    use std::sync::Arc;
    use std::thread;

    let counter = Arc::new(UserMessageCounter::new("alice".to_string()));
    let mut handles = vec![];

    for _ in 0..10 {
      let counter = Arc::clone(&counter);
      let handle = thread::spawn(move || {
        let mut ids = vec![];
        for _ in 0..100 {
          ids.push(counter.next_id());
        }
        ids
      });
      handles.push(handle);
    }

    let mut all_ids = std::collections::HashSet::new();
    for handle in handles {
      let ids = handle.join().expect("Thread panicked");
      for id in ids {
        assert!(all_ids.insert(id), "Duplicate ID");
      }
    }

    assert_eq!(all_ids.len(), 1000);
    assert_eq!(counter.current_count(), 1000);
  }

  #[test]
  fn test_cache_with_user_counter() {
    let cache = MessageCache::new(100);
    let counter = UserMessageCounter::new("alice".to_string());

    let msg1 = SharedServerMessage::new_with_counter(
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "hello".to_string(),
      },
      &counter,
    );

    let msg2 = SharedServerMessage::new_with_counter(
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "world".to_string(),
      },
      &counter,
    );

    // Different messages = different IDs
    encode_message_with_cache(&msg1, &cache).expect("Encoding failed");
    encode_message_with_cache(&msg2, &cache).expect("Encoding failed");

    assert_eq!(cache.len(), 2);
  }

  #[test]
  fn test_system_messages() {
    let msg1 = SharedServerMessage::new_system(ServerMessage::Success {
      message: "OK".to_string(),
    });
    let msg2 = SharedServerMessage::new_system(ServerMessage::Success {
      message: "Done".to_string(),
    });

    assert_ne!(msg1.cache_key(), msg2.cache_key());
  }
}
