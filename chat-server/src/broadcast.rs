use chat_core::{
  error::ApplicationError,
  protocol::{ServerMessage, encode_message},
};
use tokio::{
  io::AsyncWriteExt,
  net::tcp::OwnedWriteHalf,
  sync::broadcast::{Receiver, error::RecvError},
  task::JoinHandle,
};
use tracing::warn;

pub fn spawn_broadcast_dispatcher(
  mut rec: Receiver<ServerMessage>,
  channel_owner: String,
  mut writer: OwnedWriteHalf,
) -> JoinHandle<Result<(), ApplicationError>> {
  tokio::spawn(async move {
    let success_msg = ServerMessage::Success {
      message: format!("Welcome to the chat 🙏 `{}`", channel_owner),
    };
    let frame = encode_message(&success_msg)?;

    writer.write_all(&frame).await?;
    loop {
      match rec.recv().await {
        Ok(message) => {
          if let Some(message_username) = message.username()
            && message_username == channel_owner
          {
            continue;
          }

          match message {
            ServerMessage::Message { username, content } => {
              let frame = encode_message(&ServerMessage::Message { username, content })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::Error { reason } => {
              let frame = encode_message(&ServerMessage::Error { reason })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::Success { message } => {
              let frame = encode_message(&ServerMessage::Success { message })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::UserJoined { username } => {
              let frame = encode_message(&ServerMessage::UserJoined { username })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::UserLeft { username } => {
              let frame = encode_message(&ServerMessage::UserLeft { username })?;
              writer.write_all(&frame).await?;
            }
          }
        }
        Err(RecvError::Lagged(lagged_by)) => {
          warn!("reciever lagged by {lagged_by} messages");
        }
        Err(RecvError::Closed) => {
          warn!("Broadcast channel has been closed, Exiting");
          break;
        }
      }
    }

    let success_msg = ServerMessage::Success {
      message: format!("Disconnected: Good bye `{}`!", channel_owner),
    };
    let frame = encode_message(&success_msg)?;
    writer.write_all(&frame).await?;
    Ok(())
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::sync::broadcast;

  #[test]
  fn test_message_username_extraction() {
    let messages_with_usernames = vec![
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "Hello".to_string(),
      },
      ServerMessage::UserJoined {
        username: "bob".to_string(),
      },
      ServerMessage::UserLeft {
        username: "charlie".to_string(),
      },
    ];

    let messages_without_usernames = vec![
      ServerMessage::Success {
        message: "Welcome!".to_string(),
      },
      ServerMessage::Error {
        reason: "Error occurred".to_string(),
      },
    ];

    for message in &messages_with_usernames {
      assert!(
        message.username().is_some(),
        "Message should have a username"
      );
    }

    for message in &messages_without_usernames {
      assert!(
        message.username().is_none(),
        "Message should not have a username"
      );
    }
  }

  #[test]
  fn test_message_serialization() {
    let test_messages = vec![
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "Hello, world!".to_string(),
      },
      ServerMessage::Error {
        reason: "Test error".to_string(),
      },
      ServerMessage::Success {
        message: "Welcome to the chat!".to_string(),
      },
      ServerMessage::UserJoined {
        username: "newuser".to_string(),
      },
      ServerMessage::UserLeft {
        username: "leavinguser".to_string(),
      },
    ];

    for message in test_messages {
      let encoded = encode_message(&message).expect("Failed to encode message");
      assert!(
        encoded.len() > 4,
        "Encoded message should have length prefix and payload"
      );

      let payload = &encoded[4..];

      let decoded: ServerMessage =
        chat_core::protocol::decode_message(payload).expect("Failed to decode message");

      match (&message, decoded) {
        (
          ServerMessage::Message {
            username: orig_user,
            content: orig_content,
          },
          ServerMessage::Message {
            username: dec_user,
            content: dec_content,
          },
        ) => {
          assert_eq!(orig_user, &dec_user);
          assert_eq!(orig_content, &dec_content);
        }
        (
          ServerMessage::Error {
            reason: orig_reason,
          },
          ServerMessage::Error { reason: dec_reason },
        ) => {
          assert_eq!(orig_reason, &dec_reason);
        }
        (
          ServerMessage::Success { message: orig_msg },
          ServerMessage::Success { message: dec_msg },
        ) => {
          assert_eq!(orig_msg, &dec_msg);
        }
        (
          ServerMessage::UserJoined {
            username: orig_user,
          },
          ServerMessage::UserJoined { username: dec_user },
        ) => {
          assert_eq!(orig_user, &dec_user);
        }
        (
          ServerMessage::UserLeft {
            username: orig_user,
          },
          ServerMessage::UserLeft { username: dec_user },
        ) => {
          assert_eq!(orig_user, &dec_user);
        }
        _ => panic!("Message type mismatch after encoding/decoding"),
      }
    }
  }

  #[test]
  fn test_large_message_serialization() {
    let large_content = "x".repeat(10000);
    let test_message = ServerMessage::Message {
      username: "largeuser".to_string(),
      content: large_content,
    };

    let encoded = encode_message(&test_message).expect("Failed to encode large message");
    assert!(
      encoded.len() > 10000,
      "Large message should result in substantial encoded size"
    );

    let payload = &encoded[4..];

    let decoded: ServerMessage =
      chat_core::protocol::decode_message(payload).expect("Failed to decode large message");

    match decoded {
      ServerMessage::Message { username, content } => {
        assert_eq!(username, "largeuser");
        assert_eq!(content.len(), 10000);
      }
      _ => panic!("Expected large ServerMessage::Message"),
    }
  }

  #[test]
  fn test_empty_message_serialization() {
    let test_message = ServerMessage::Message {
      username: "".to_string(),
      content: "".to_string(),
    };

    let encoded = encode_message(&test_message).expect("Failed to encode empty message");
    assert!(
      encoded.len() >= 4,
      "Message should have at least length prefix"
    );

    let payload = &encoded[4..];

    let decoded: ServerMessage =
      chat_core::protocol::decode_message(payload).expect("Failed to decode empty message");

    match decoded {
      ServerMessage::Message { username, content } => {
        assert_eq!(username, "");
        assert_eq!(content, "");
      }
      _ => panic!("Expected empty ServerMessage::Message"),
    }
  }

  #[test]
  fn test_broadcast_dispatcher_creation() {
    let (tx, _rx) = broadcast::channel(10);

    let test_message = ServerMessage::Success {
      message: "Test".to_string(),
    };

    let result = tx.send(test_message);
    assert!(
      result.is_ok(),
      "Should be able to send message through broadcast channel"
    );
  }

  #[test]
  fn test_message_equality() {
    let msg1 = ServerMessage::Message {
      username: "alice".to_string(),
      content: "Hello".to_string(),
    };

    let msg2 = ServerMessage::Message {
      username: "alice".to_string(),
      content: "Hello".to_string(),
    };

    let encoded1 = encode_message(&msg1).expect("Failed to encode first message");
    let encoded2 = encode_message(&msg2).expect("Failed to encode second message");

    assert_eq!(
      encoded1, encoded2,
      "Identical messages should produce identical encoded data"
    );
  }
}
