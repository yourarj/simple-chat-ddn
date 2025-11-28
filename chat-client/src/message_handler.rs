use anyhow::anyhow;
use chat_core::protocol::{LENGTH_PREFIX, ServerMessage, decode_message};

pub async fn handle_server_message(data: &[u8]) -> anyhow::Result<()> {
  if data.len() < LENGTH_PREFIX {
    return Err(anyhow!("Message too short"));
  }

  let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
  if data.len() < LENGTH_PREFIX + length {
    return Err(anyhow!("Incomplete message"));
  }

  let message = decode_message(&data[LENGTH_PREFIX..LENGTH_PREFIX + length])
    .map_err(|e| anyhow!("Failed to decode server message: {}", e))?;

  match message {
    ServerMessage::Message { username, content } => {
      println!("🗨️: {}: {}", username, content);
    }
    ServerMessage::Error { reason } => {
      println!("❌: {}", reason);
    }
    ServerMessage::UserNameAlreadyTaken { username } => {
      println!("❌: username `{}` not available", username);
    }
    ServerMessage::Success { message } => {
      println!("💐: {}", message);
    }
    ServerMessage::UserJoined { username } => {
      println!("📢: `{}` joined the chat", username);
    }
    ServerMessage::UserLeft { username } => {
      println!("📢: `{}` left the chat", username);
    }
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use chat_core::protocol::{ServerMessage, encode_message};

  #[tokio::test]
  async fn test_handle_server_message_message() {
    let message = ServerMessage::Message {
      username: "alice".to_string(),
      content: "Hello everyone!".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_error() {
    let message = ServerMessage::Error {
      reason: "Test error".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_success() {
    let message = ServerMessage::Success {
      message: "Welcome to chat!".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_user_joined() {
    let message = ServerMessage::UserJoined {
      username: "bob".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_user_left() {
    let message = ServerMessage::UserLeft {
      username: "charlie".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_username_not_found() {
    let message = ServerMessage::UserNameAlreadyTaken {
      username: "takenuser".to_string(),
    };

    let encoded = encode_message(&message).unwrap();
    let result = handle_server_message(&encoded).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn test_handle_server_message_invalid_data() {
    let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
    let result = handle_server_message(&invalid_data).await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn test_handle_server_message_short_data() {
    let short_data = vec![0x00, 0x00];
    let result = handle_server_message(&short_data).await;
    assert!(result.is_err());
  }
}
