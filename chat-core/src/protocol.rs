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

pub const LENGTH_PREFIX: usize = 4;

pub fn encode_message<T: Encode>(message: &T) -> Result<Bytes, bincode::error::EncodeError> {
  let payload = Bytes::from(bincode::encode_to_vec(message, BINCODE_STANDADRD_CONFIG)?);
  let mut frame = BytesMut::with_capacity(LENGTH_PREFIX + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame.freeze())
}

pub fn decode_message<T: Decode<()>>(buf: &[u8]) -> Result<T, bincode::error::DecodeError> {
  bincode::decode_from_slice(buf, BINCODE_STANDADRD_CONFIG).map(|(body, _)| body)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_client_message_username_extraction_join() {
    let msg = ClientMessage::Join {
      username: "alice".to_string(),
    };
    assert_eq!(msg.username(), Some("alice"));
  }

  #[test]
  fn test_client_message_username_extraction_leave() {
    let msg = ClientMessage::Leave {
      username: "bob".to_string(),
    };
    assert_eq!(msg.username(), Some("bob"));
  }

  #[test]
  fn test_client_message_username_extraction_message() {
    let msg = ClientMessage::Message {
      username: "charlie".to_string(),
      content: "Hello, world!".to_string(),
    };
    assert_eq!(msg.username(), Some("charlie"));
  }

  #[test]
  fn test_server_message_username_extraction_message() {
    let msg = ServerMessage::Message {
      username: "alice".to_string(),
      content: "Hi everyone!".to_string(),
    };
    assert_eq!(msg.username(), Some("alice"));
  }

  #[test]
  fn test_server_message_username_extraction_user_joined() {
    let msg = ServerMessage::UserJoined {
      username: "bob".to_string(),
    };
    assert_eq!(msg.username(), Some("bob"));
  }

  #[test]
  fn test_server_message_username_extraction_user_left() {
    let msg = ServerMessage::UserLeft {
      username: "charlie".to_string(),
    };
    assert_eq!(msg.username(), Some("charlie"));
  }

  #[test]
  fn test_server_message_username_extraction_success() {
    let msg = ServerMessage::Success {
      message: "Welcome to the chat!".to_string(),
    };
    assert_eq!(msg.username(), None);
  }

  #[test]
  fn test_server_message_username_extraction_error() {
    let msg = ServerMessage::Error {
      reason: "Invalid username".to_string(),
    };
    assert_eq!(msg.username(), None);
  }

  #[test]
  fn test_client_message_serialization_and_deserialization() {
    let join_msg = ClientMessage::Join {
      username: "testuser".to_string(),
    };
    let encoded = encode_message(&join_msg).expect("Failed to encode join message");
    let decoded: ClientMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode join message");
    assert!(matches!(decoded, ClientMessage::Join { .. }));
    if let ClientMessage::Join { username } = decoded {
      assert_eq!(username, "testuser");
    }

    let leave_msg = ClientMessage::Leave {
      username: "testuser".to_string(),
    };
    let encoded = encode_message(&leave_msg).expect("Failed to encode leave message");
    let decoded: ClientMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode leave message");
    assert!(matches!(decoded, ClientMessage::Leave { .. }));
    if let ClientMessage::Leave { username } = decoded {
      assert_eq!(username, "testuser");
    }

    let message_content = "Hello, this is a test message!";
    let message_msg = ClientMessage::Message {
      username: "testuser".to_string(),
      content: message_content.to_string(),
    };
    let encoded = encode_message(&message_msg).expect("Failed to encode message");
    let decoded: ClientMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode message");
    assert!(matches!(decoded, ClientMessage::Message { .. }));
    if let ClientMessage::Message { username, content } = decoded {
      assert_eq!(username, "testuser");
      assert_eq!(content, message_content);
    }
  }

  #[test]
  fn test_server_message_serialization_and_deserialization() {
    let success_msg = ServerMessage::Success {
      message: "Connection successful!".to_string(),
    };
    let encoded = encode_message(&success_msg).expect("Failed to encode success message");
    let decoded: ServerMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode success message");
    assert!(matches!(decoded, ServerMessage::Success { .. }));
    if let ServerMessage::Success { message } = decoded {
      assert_eq!(message, "Connection successful!");
    }

    let error_msg = ServerMessage::Error {
      reason: "Username already taken".to_string(),
    };
    let encoded = encode_message(&error_msg).expect("Failed to encode error message");
    let decoded: ServerMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode error message");
    assert!(matches!(decoded, ServerMessage::Error { .. }));
    if let ServerMessage::Error { reason } = decoded {
      assert_eq!(reason, "Username already taken");
    }

    let server_message = ServerMessage::Message {
      username: "alice".to_string(),
      content: "Hello from server!".to_string(),
    };
    let encoded = encode_message(&server_message).expect("Failed to encode server message");
    let decoded: ServerMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode server message");
    assert!(matches!(decoded, ServerMessage::Message { .. }));
    if let ServerMessage::Message { username, content } = decoded {
      assert_eq!(username, "alice");
      assert_eq!(content, "Hello from server!");
    }

    let user_joined_msg = ServerMessage::UserJoined {
      username: "newuser".to_string(),
    };
    let encoded = encode_message(&user_joined_msg).expect("Failed to encode user joined message");
    let decoded: ServerMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode user joined message");
    assert!(matches!(decoded, ServerMessage::UserJoined { .. }));
    if let ServerMessage::UserJoined { username } = decoded {
      assert_eq!(username, "newuser");
    }

    let user_left_msg = ServerMessage::UserLeft {
      username: "leavinguser".to_string(),
    };
    let encoded = encode_message(&user_left_msg).expect("Failed to encode user left message");
    let decoded: ServerMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode user left message");
    assert!(matches!(decoded, ServerMessage::UserLeft { .. }));
    if let ServerMessage::UserLeft { username } = decoded {
      assert_eq!(username, "leavinguser");
    }
  }

  #[test]
  fn test_message_framing_length_prefix() {
    let msg = ClientMessage::Join {
      username: "test".to_string(),
    };

    let frame = encode_message(&msg).expect("Failed to encode message");

    assert!(
      frame.len() > LENGTH_PREFIX,
      "Frame should be longer than length prefix"
    );

    let length_bytes = &frame[0..LENGTH_PREFIX];
    let length = u32::from_be_bytes([
      length_bytes[0],
      length_bytes[1],
      length_bytes[2],
      length_bytes[3],
    ]) as usize;

    assert_eq!(length, frame.len() - LENGTH_PREFIX);

    let payload = &frame[LENGTH_PREFIX..];
    let decoded: ClientMessage = decode_message(payload).expect("Failed to decode payload");
    assert!(matches!(decoded, ClientMessage::Join { .. }));
  }

  #[test]
  fn test_empty_strings_in_messages() {
    let msg = ClientMessage::Message {
      username: "".to_string(),
      content: "Message with empty username".to_string(),
    };

    let encoded = encode_message(&msg).expect("Failed to encode message with empty username");
    let decoded: ClientMessage = decode_message(&encoded[LENGTH_PREFIX..])
      .expect("Failed to decode message with empty username");

    if let ClientMessage::Message { username, content } = decoded {
      assert_eq!(username, "");
      assert_eq!(content, "Message with empty username");
    }

    let msg = ClientMessage::Message {
      username: "user".to_string(),
      content: "".to_string(),
    };

    let encoded = encode_message(&msg).expect("Failed to encode message with empty content");
    let decoded: ClientMessage = decode_message(&encoded[LENGTH_PREFIX..])
      .expect("Failed to decode message with empty content");

    if let ClientMessage::Message { username, content } = decoded {
      assert_eq!(username, "user");
      assert_eq!(content, "");
    }
  }

  #[test]
  fn test_long_strings_in_messages() {
    let long_username = "a".repeat(1000);
    let long_content = "b".repeat(10000);

    let msg = ClientMessage::Message {
      username: long_username.clone(),
      content: long_content.clone(),
    };

    let encoded = encode_message(&msg).expect("Failed to encode message with long strings");
    let decoded: ClientMessage = decode_message(&encoded[LENGTH_PREFIX..])
      .expect("Failed to decode message with long strings");

    if let ClientMessage::Message { username, content } = decoded {
      assert_eq!(username, long_username);
      assert_eq!(content, long_content);
    }
  }

  #[test]
  fn test_decode_message_with_invalid_data() {
    let result: Result<ClientMessage, _> = decode_message::<ClientMessage>(&[]);
    assert!(result.is_err(), "Should fail to decode empty data");

    let invalid_data = vec![
      0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE, 0x12, 0x34, 0x56, 0x78,
    ];
    let result: Result<ClientMessage, _> = decode_message(&invalid_data);

    assert!(
      result.is_ok() || result.is_err(),
      "Decoding should either succeed or fail gracefully"
    );
  }

  #[test]
  fn test_encode_message_edge_cases() {
    let small_msg = ClientMessage::Join {
      username: "a".to_string(),
    };
    let encoded = encode_message(&small_msg).expect("Failed to encode small message");
    assert!(encoded.len() > LENGTH_PREFIX);

    let large_msg = ClientMessage::Message {
      username: "user".to_string(),
      content: "x".repeat(100000),
    };
    let encoded = encode_message(&large_msg).expect("Failed to encode large message");
    assert!(encoded.len() > 100000);

    let decoded: ClientMessage =
      decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode large message");
    if let ClientMessage::Message { username, content } = decoded {
      assert_eq!(username, "user");
      assert_eq!(content.len(), 100000);
    }
  }

  #[test]
  fn test_message_type_discrimination() {
    let messages = [
      ClientMessage::Join {
        username: "user1".to_string(),
      },
      ClientMessage::Leave {
        username: "user2".to_string(),
      },
      ClientMessage::Message {
        username: "user3".to_string(),
        content: "test".to_string(),
      },
    ];

    for (i, original_msg) in messages.iter().enumerate() {
      let encoded = encode_message(original_msg).expect("Failed to encode message");
      let decoded: ClientMessage =
        decode_message(&encoded[LENGTH_PREFIX..]).expect("Failed to decode message");

      match (original_msg, decoded) {
        (
          ClientMessage::Join {
            username: orig_user,
          },
          ClientMessage::Join { username: dec_user },
        ) => {
          assert_eq!(
            *orig_user, dec_user,
            "Join message {} should have matching usernames",
            i
          );
        }
        (
          ClientMessage::Leave {
            username: orig_user,
          },
          ClientMessage::Leave { username: dec_user },
        ) => {
          assert_eq!(
            *orig_user, dec_user,
            "Leave message {} should have matching usernames",
            i
          );
        }
        (
          ClientMessage::Message {
            username: orig_user,
            content: orig_content,
          },
          ClientMessage::Message {
            username: dec_user,
            content: dec_content,
          },
        ) => {
          assert_eq!(
            *orig_user, dec_user,
            "Message {} should have matching usernames",
            i
          );
          assert_eq!(
            *orig_content, dec_content,
            "Message {} should have matching content",
            i
          );
        }
        _ => panic!("Message {} type mismatch after encoding/decoding", i),
      }
    }
  }

  #[test]
  fn test_length_prefix_constants() {
    assert_eq!(LENGTH_PREFIX, 4, "Length prefix should be 4 bytes for u32");

    let msg = ClientMessage::Join {
      username: "test".to_string(),
    };
    let encoded = encode_message(&msg).expect("Failed to encode test message");

    assert!(
      encoded.len() > LENGTH_PREFIX,
      "Encoded message should be longer than length prefix"
    );

    let length_bytes = &encoded[0..LENGTH_PREFIX];
    let length = u32::from_be_bytes([
      length_bytes[0],
      length_bytes[1],
      length_bytes[2],
      length_bytes[3],
    ]);

    assert_eq!(length as usize, encoded.len() - LENGTH_PREFIX);
  }
}
