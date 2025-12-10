//! Client-side username handling and authentication
//!
//! This module manages the client's username authentication process
//! including sending join requests and handling server responses.

use chat_core::{
  error::{ApplicationError, Result},
  protocol::{
    ClientMessage, LENGTH_PREFIX, MAX_MESSAGE_SIZE, ServerMessage, decode_message, encode_message,
  },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tracing::{debug, error, info, warn};

const MAX_JOIN_RETRIES: usize = 3;

/// Result of a join attempt
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinResult {
  /// Successfully joined with username
  Success,
  /// Username already taken, should retry with different name
  UsernameTaken,
  /// Invalid username format
  InvalidUsername(String),
  /// Server error occurred
  ServerError(String),
  /// Connection closed
  ConnectionClosed,
}

/// Send join request to server
///
/// # Arguments
/// * `writer` - TCP writer to send to
/// * `username` - Username to register with
pub async fn send_join_request(
  writer: &mut BufWriter<OwnedWriteHalf>,
  username: &str,
) -> Result<()> {
  debug!("Sending join request for username: {}", username);

  let join_msg = ClientMessage::Join {
    username: username.to_string(),
  };

  let frame = encode_message(&join_msg)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;

  info!("Join request sent for: {}", username);
  Ok(())
}

/// Read and parse server response to join request
///
/// # Arguments
/// * `reader` - TCP reader to receive from
pub async fn read_join_response(reader: &mut BufReader<OwnedReadHalf>) -> Result<JoinResult> {
  match read_server_message(reader).await {
    Ok(ServerMessage::Success { message }) => {
      info!("Join successful: {}", message);
      println!("\n✓ {}", message);
      Ok(JoinResult::Success)
    }
    Ok(ServerMessage::Error { reason }) => {
      warn!("Join failed: {}", reason);
      println!("\n✗ Join failed: {}", reason);

      // Parse error type
      if reason.contains("already taken") {
        Ok(JoinResult::UsernameTaken)
      } else if reason.contains("Invalid username") {
        Ok(JoinResult::InvalidUsername(reason))
      } else {
        Ok(JoinResult::ServerError(reason))
      }
    }
    Ok(other) => {
      warn!("Unexpected response to join request: {:?}", other);
      Ok(JoinResult::ServerError(
        "Unexpected server response".to_string(),
      ))
    }
    Err(ApplicationError::ClientReadStreamClosed) => {
      error!("Server closed connection during join");
      Ok(JoinResult::ConnectionClosed)
    }
    Err(e) => {
      error!("Error reading join response: {}", e);
      Err(e)
    }
  }
}

/// Perform complete join handshake with server
///
/// Sends join request and waits for success response, with retry logic
///
/// # Arguments
/// * `reader` - TCP reader
/// * `writer` - TCP writer
/// * `username` - Username to authenticate with
pub async fn perform_join_handshake(
  reader: &mut BufReader<OwnedReadHalf>,
  writer: &mut BufWriter<OwnedWriteHalf>,
  username: &str,
) -> Result<()> {
  let mut attempts = 0;

  loop {
    if attempts >= MAX_JOIN_RETRIES {
      error!("Maximum join attempts ({}) exceeded", MAX_JOIN_RETRIES);
      return Err(ApplicationError::config_error(
        "Failed to join after maximum attempts".to_string(),
      ));
    }

    attempts += 1;
    debug!("Join attempt {}/{}", attempts, MAX_JOIN_RETRIES);

    // Send join request
    send_join_request(writer, username).await?;

    // Wait for response
    match read_join_response(reader).await? {
      JoinResult::Success => {
        info!("Successfully joined as '{}'", username);
        return Ok(());
      }
      JoinResult::UsernameTaken => {
        error!("Username '{}' is already taken", username);
        return Err(ApplicationError::invalid_username(
          username.to_string(),
          "Username already in use".to_string(),
        ));
      }
      JoinResult::InvalidUsername(reason) => {
        error!("Invalid username '{}': {}", username, reason);
        return Err(ApplicationError::invalid_username(
          username.to_string(),
          reason,
        ));
      }
      JoinResult::ServerError(reason) => {
        warn!("Server error during join: {}", reason);
        if attempts < MAX_JOIN_RETRIES {
          println!("Retrying...");
          tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
          continue;
        } else {
          return Err(ApplicationError::config_error(format!(
            "Server error: {}",
            reason
          )));
        }
      }
      JoinResult::ConnectionClosed => {
        error!("Connection closed by server");
        return Err(ApplicationError::ClientReadStreamClosed);
      }
    }
  }
}

/// Read a server message from the stream
pub async fn read_server_message(reader: &mut BufReader<OwnedReadHalf>) -> Result<ServerMessage> {
  // Read length prefix
  let mut length_bytes = [0u8; LENGTH_PREFIX];
  let n = reader.read_exact(&mut length_bytes).await?;

  if n == 0 {
    return Err(ApplicationError::ClientReadStreamClosed);
  }

  if n < LENGTH_PREFIX {
    return Err(ApplicationError::invalid_frame(format!(
      "Incomplete length prefix: got {} bytes, expected {}",
      n, LENGTH_PREFIX
    )));
  }

  let payload_len = u32::from_be_bytes(length_bytes) as usize;

  if payload_len > MAX_MESSAGE_SIZE {
    return Err(ApplicationError::message_too_large(
      payload_len,
      MAX_MESSAGE_SIZE,
    ));
  }

  if payload_len == 0 {
    return Err(ApplicationError::invalid_frame(
      "Zero-length payload".to_string(),
    ));
  }

  // Read payload
  let mut payload = vec![0u8; payload_len];
  reader.read_exact(&mut payload).await?;

  decode_message(&payload)
}

/// Send leave request to server
///
/// # Arguments
/// * `writer` - TCP writer to send to
/// * `username` - Username leaving the chat
pub async fn send_leave_request(
  writer: &mut BufWriter<OwnedWriteHalf>,
  username: &str,
) -> Result<()> {
  info!("Sending leave request for: {}", username);

  let leave_msg = ClientMessage::Leave {
    username: username.to_string(),
  };

  let frame = encode_message(&leave_msg)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;

  debug!("Leave request sent");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_join_result_equality() {
    assert_eq!(JoinResult::Success, JoinResult::Success);
    assert_eq!(JoinResult::UsernameTaken, JoinResult::UsernameTaken);
    assert_ne!(JoinResult::Success, JoinResult::UsernameTaken);
  }

  #[test]
  fn test_join_result_types() {
    let invalid = JoinResult::InvalidUsername("too short".to_string());
    match invalid {
      JoinResult::InvalidUsername(reason) => {
        assert_eq!(reason, "too short");
      }
      _ => panic!("Expected InvalidUsername"),
    }
  }
}
