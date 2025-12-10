//! Integration tests for chat-core
//! Tests protocol encoding/decoding and core functionality

use chat_core::{
  message_cache::MessageCache,
  protocol::{
    ServerMessage, SharedServerMessage, UserMessageCounter, decode_message, encode_message,
  },
};

#[test]
fn test_message_encoding_decoding() {
  let msg = ServerMessage::Message {
    username: "alice".to_string(),
    content: "Hello".to_string(),
  };

  let encoded = encode_message(&msg).expect("Encoding failed");
  assert!(encoded.len() > 4);

  let payload = &encoded[4..];
  let decoded: ServerMessage = decode_message(payload).expect("Decoding failed");
  assert_eq!(msg, decoded);
}

#[test]
fn test_user_message_counter() {
  let counter = UserMessageCounter::new("alice".to_string());

  let id1 = counter.next_id();
  let id2 = counter.next_id();
  let id3 = counter.next_id();

  assert_ne!(id1, id2);
  assert_ne!(id2, id3);
  assert_eq!(counter.current_count(), 3);
}

#[test]
fn test_message_cache() {
  let cache = MessageCache::new(100);
  let counter = UserMessageCounter::new("alice".to_string());

  let msg = SharedServerMessage::new_with_counter(
    ServerMessage::Message {
      username: "alice".to_string(),
      content: "test".to_string(),
    },
    &counter,
  );

  let cache_key = msg.cache_key();
  cache.put(cache_key, bytes::Bytes::from("test data"));

  assert_eq!(cache.get(cache_key), Some(bytes::Bytes::from("test data")));
  assert_eq!(cache.len(), 1);
}

#[test]
fn test_system_messages() {
  let msg1 = SharedServerMessage::new_system(ServerMessage::Success {
    message: "OK".to_string(),
  });
  let msg2 = SharedServerMessage::new_system(ServerMessage::Success {
    message: "Done".to_string(),
  });

  // Different system messages should have different IDs
  assert_ne!(msg1.cache_key(), msg2.cache_key());
}
