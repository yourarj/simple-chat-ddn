// common/src/lib.rs
use bincode::{self, Decode, Encode, config::Configuration};
use bytes::{Bytes, BytesMut};

const BINCODE_STANDADRD_CONFIG: Configuration = bincode::config::standard();

#[derive(Clone, Encode, Decode)]
pub enum ClientMessage {
  Join { username: String },
  Leave { username: String },
  Message { username: String, content: String },
}

impl ClientMessage {
  pub fn username(&self) -> Option<&str> {
    match self {
      ClientMessage::Join { username, .. }
      | ClientMessage::Leave { username }
      | ClientMessage::Message {
        username,
        content: _,
      } => Some(username),
    }
  }
}

#[derive(Clone, Encode, Decode)]
pub enum ServerMessage {
  Success { message: String },
  Error { reason: String },
  Message { username: String, content: String },
  UserJoined { username: String },
  UserLeft { username: String },
}

impl ServerMessage {
  pub fn username(&self) -> Option<&str> {
    match self {
      ServerMessage::Message { username, .. }
      | ServerMessage::UserJoined { username }
      | ServerMessage::UserLeft { username } => Some(username),
      _ => None,
    }
  }
}

// frame size
pub const LENGTH_PREFIX: usize = 4;

/// Encode
pub fn encode_message<T: Encode>(message: &T) -> Result<Bytes, bincode::error::EncodeError> {
  let payload = Bytes::from(bincode::encode_to_vec(message, BINCODE_STANDADRD_CONFIG)?);
  let mut frame = BytesMut::with_capacity(LENGTH_PREFIX + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame.freeze())
}

/// Decode
pub fn decode_message<T: Decode<()>>(buf: &[u8]) -> Result<T, bincode::error::DecodeError> {
  bincode::decode_from_slice(buf, BINCODE_STANDADRD_CONFIG).map(|(body, _)| body)
}

#[cfg(test)]
mod tests {
  //   use super::*;

  //   #[test]
  //   fn test_client_message_serialization() {
  //     let msg = ClientMessage::Message {
  //       username: "test".to_string(),
  //       content: "hello".to_string(),
  //     };

  //     let bytes = msg.to_bytes().unwrap();
  //     let decoded = ChatMessage::from_bytes(&bytes).unwrap();

  //     match decoded {
  //       ChatMessage::Message { username, content } => {
  //         assert_eq!(username, "test");
  //         assert_eq!(content, "hello");
  //       }
  //       _ => panic!("Unexpected message type"),
  //     }
  //   }

  //   #[test]
  //   fn test_framing() {
  //     let msg = ChatMessage::Join {
  //       username: "user".to_string(),
  //     };

  //     let frame = encode_message(&msg).unwrap();
  //     assert!(frame.len() > LENGTH_PREFIX);

  //     // Test decoding without length prefix
  //     let decoded = decode_message(&frame[LENGTH_PREFIX..]).unwrap();
  //     assert!(matches!(decoded, ChatMessage::Join { .. }));
  //   }
}
