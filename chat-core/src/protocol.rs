// common/src/lib.rs
use bincode::{self, Decode, Encode, config::Configuration};
use bytes::{Bytes, BytesMut};

const BINCODE_STANDADRD_CONFIG: Configuration = bincode::config::standard();

/// Chat protocol messages exchanged between client and server
#[derive(Debug, Clone, Encode, Decode)]
pub enum ChatMessage {
  Join { username: String },
  Leave { username: String },
  Message { username: String, content: String },
  Error { reason: String },
  Success { message: String },
}

impl ChatMessage {
  /// Serialize message to bytes for network transmission
  pub fn to_bytes(&self) -> Result<Bytes, bincode::error::EncodeError> {
    let encoded = bincode::encode_to_vec(self, BINCODE_STANDADRD_CONFIG)?;
    Ok(Bytes::from(encoded))
  }

  /// Deserialize message from bytes
  pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::error::DecodeError> {
    bincode::decode_from_slice(bytes, BINCODE_STANDADRD_CONFIG).map(|(body, _)| body)
  }

  /// Get username from message if present
  pub fn username(&self) -> Option<&str> {
    match self {
      ChatMessage::Join { username } => Some(username),
      ChatMessage::Leave { username } => Some(username),
      ChatMessage::Message { username, .. } => Some(username),
      _ => None,
    }
  }
}

/// Frame size for message length prefix
pub const LENGTH_PREFIX: usize = 4;

/// Encode a message with length prefix for framing
pub fn encode_message(message: &ChatMessage) -> Result<Bytes, bincode::error::EncodeError> {
  let payload = message.to_bytes()?;
  let mut frame = BytesMut::with_capacity(LENGTH_PREFIX + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame.freeze())
}

/// Decode a message from framed bytes
pub fn decode_message(buf: &[u8]) -> Result<ChatMessage, bincode::error::DecodeError> {
  ChatMessage::from_bytes(buf)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_message_serialization() {
    let msg = ChatMessage::Message {
      username: "test".to_string(),
      content: "hello".to_string(),
    };

    let bytes = msg.to_bytes().unwrap();
    let decoded = ChatMessage::from_bytes(&bytes).unwrap();

    match decoded {
      ChatMessage::Message { username, content } => {
        assert_eq!(username, "test");
        assert_eq!(content, "hello");
      }
      _ => panic!("Unexpected message type"),
    }
  }

  #[test]
  fn test_framing() {
    let msg = ChatMessage::Join {
      username: "user".to_string(),
    };

    let frame = encode_message(&msg).unwrap();
    assert!(frame.len() > LENGTH_PREFIX);

    // Test decoding without length prefix
    let decoded = decode_message(&frame[LENGTH_PREFIX..]).unwrap();
    assert!(matches!(decoded, ChatMessage::Join { .. }));
  }
}
