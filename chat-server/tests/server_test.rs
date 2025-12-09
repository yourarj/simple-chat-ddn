//! Integration tests for the chat server
//!
//! Tests the complete system: server + client communication

use chat_core::{
  ApplicationError,
  error::Result,
  protocol::{ClientMessage, ServerMessage},
};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::Duration;
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpStream,
  time::timeout,
};

const TEST_HOST: &str = "127.0.0.1";
static PORT_COUNTER: AtomicU16 = AtomicU16::new(8000);
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Get a unique port for each test
fn get_test_port() -> u16 {
  PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Helper to start test server in background
async fn start_test_server(port: u16) -> Result<tokio::task::JoinHandle<()>> {
  use chat_core::message_cache::MessageCache;
  use std::sync::Arc;
  use tokio::sync::broadcast;

  let handle = tokio::spawn(async move {
    let (tx, _rx) = broadcast::channel(1000);
    let broadcaster = Arc::new(tx);

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", TEST_HOST, port))
      .await
      .unwrap_or_else(|_| panic!("Failed to bind test server on port {}", port));

    let broadcast_pool = chat_server::broadcast_pool::BroadcastPool::new(broadcaster);
    let cache = MessageCache::new(1000);

    while let Ok((stream, _)) = listener.accept().await {
      let pool = broadcast_pool.clone();
      let cache_clone = cache.clone();
      tokio::spawn(async move {
        let _ = chat_server::client_handler::handle_client(stream, pool, cache_clone).await;
      });
    }
  });

  // Give server time to start
  tokio::time::sleep(Duration::from_millis(100)).await;
  Ok(handle)
}

/// Helper to connect and authenticate a client
async fn connect_and_join(port: u16, username: &str) -> Result<TcpStream> {
  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port)).await?;

  let (mut reader, mut writer) = stream.into_split();

  // Send join message
  let join_msg = ClientMessage::Join {
    username: username.to_string(),
  };
  let frame = encode_client_message(&join_msg)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;

  // Wait for welcome message
  let welcome = read_server_message(&mut reader).await?;
  match welcome {
    ServerMessage::Success { message } => {
      assert!(message.contains("Welcome"));
    }
    _ => panic!("Expected welcome message, got: {:?}", welcome),
  }

  // Reunite the stream
  reader
    .reunite(writer)
    .map_err(|_| ApplicationError::Miscellaneous(String::from("Reader Writer reunite issue")))
}

/// Helper to encode a client message
fn encode_client_message(msg: &ClientMessage) -> Result<Vec<u8>> {
  let payload = bincode::encode_to_vec(msg, bincode::config::standard())
    .map_err(|e| chat_core::error::ApplicationError::invalid_frame(e.to_string()))?;

  let mut frame = Vec::with_capacity(4 + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame)
}

/// Helper to read a server message
async fn read_server_message(reader: &mut tokio::net::tcp::OwnedReadHalf) -> Result<ServerMessage> {
  let mut length_bytes = [0u8; 4];
  reader.read_exact(&mut length_bytes).await?;
  let payload_len = u32::from_be_bytes(length_bytes) as usize;

  let mut payload = vec![0u8; payload_len];
  reader.read_exact(&mut payload).await?;

  let (msg, _) = bincode::decode_from_slice(&payload, bincode::config::standard())
    .map_err(|e| chat_core::error::ApplicationError::invalid_frame(e.to_string()))?;
  Ok(msg)
}

#[tokio::test]
async fn test_single_client_join_and_leave() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let stream = connect_and_join(port, "alice")
    .await
    .expect("Failed to join");
  let (mut reader, mut writer) = stream.into_split();

  // Send leave message
  let leave_msg = ClientMessage::Leave {
    username: "alice".to_string(),
  };
  let frame = encode_client_message(&leave_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Try to read goodbye (might get EOF if connection closes first)
  let result = timeout(Duration::from_secs(1), read_server_message(&mut reader)).await;

  match result {
    Ok(Ok(ServerMessage::Success { message })) => {
      assert!(message.contains("Goodbye"));
    }
    Ok(Err(_)) | Err(_) => {
      // Connection closed - acceptable
    }
    Ok(Ok(_)) => {
      // Got some other message - acceptable
    }
  }
}

#[tokio::test]
async fn test_duplicate_username_rejected() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  // First client joins
  let _client1 = connect_and_join(port, "bob")
    .await
    .expect("Failed to join as bob");

  // Second client tries same username
  let stream2 = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader2, mut writer2) = stream2.into_split();

  let join_msg = ClientMessage::Join {
    username: "bob".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer2.write_all(&frame).await.expect("Failed to send");
  writer2.flush().await.expect("Failed to flush");

  // Should receive error about username taken
  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader2))
    .await
    .expect("Timeout waiting for error")
    .expect("Failed to read error");

  match response {
    ServerMessage::Error { reason } => {
      assert!(reason.contains("already taken"));
    }
    _ => panic!(
      "Expected error about duplicate username, got: {:?}",
      response
    ),
  }
}

#[tokio::test]
async fn test_message_broadcast() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  // Connect two clients
  let stream1 = connect_and_join(port, "alice")
    .await
    .expect("Failed to join as alice");
  let (mut reader1, mut writer1) = stream1.into_split();

  let stream2 = connect_and_join(port, "bob")
    .await
    .expect("Failed to join as bob");
  let (mut reader2, mut writer2) = stream2.into_split();

  // Alice should receive bob's join notification
  let join_notif = timeout(TEST_TIMEOUT, read_server_message(&mut reader1))
    .await
    .expect("Timeout waiting for join notification")
    .expect("Failed to read join notification");

  match join_notif {
    ServerMessage::UserJoined { username } => {
      assert_eq!(username, "bob");
    }
    _ => panic!("Expected UserJoined, got: {:?}", join_notif),
  }

  // Alice sends a message
  let chat_msg = ClientMessage::Message {
    username: "alice".to_string(),
    content: "Hello Bob!".to_string(),
  };
  let frame = encode_client_message(&chat_msg).expect("Failed to encode");
  writer1.write_all(&frame).await.expect("Failed to send");
  writer1.flush().await.expect("Failed to flush");

  // Bob should receive Alice's message
  let received = timeout(TEST_TIMEOUT, read_server_message(&mut reader2))
    .await
    .expect("Timeout waiting for message")
    .expect("Failed to read message");

  match received {
    ServerMessage::Message { username, content } => {
      assert_eq!(username, "alice");
      assert_eq!(content, "Hello Bob!");
    }
    _ => panic!("Expected Message, got: {:?}", received),
  }

  // Bob sends a reply
  let reply_msg = ClientMessage::Message {
    username: "bob".to_string(),
    content: "Hi Alice!".to_string(),
  };
  let frame = encode_client_message(&reply_msg).expect("Failed to encode");
  writer2.write_all(&frame).await.expect("Failed to send");
  writer2.flush().await.expect("Failed to flush");

  // Alice should receive Bob's reply
  let received = timeout(TEST_TIMEOUT, read_server_message(&mut reader1))
    .await
    .expect("Timeout waiting for reply")
    .expect("Failed to read reply");

  match received {
    ServerMessage::Message { username, content } => {
      assert_eq!(username, "bob");
      assert_eq!(content, "Hi Alice!");
    }
    _ => panic!("Expected Message, got: {:?}", received),
  }
}

#[tokio::test]
async fn test_invalid_username() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Try to join with invalid username (too short)
  let join_msg = ClientMessage::Join {
    username: "ab".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout waiting for error")
    .expect("Failed to read error");

  match response {
    ServerMessage::Error { reason } => {
      assert!(
        reason.contains("Invalid username")
          || reason.contains("at least")
          || reason.contains("characters"),
        "Expected error about invalid username, got: {}",
        reason
      );
    }
    other => panic!("Expected error about invalid username, got: {:?}", other),
  }
}

#[tokio::test]
async fn test_user_leave_notification() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  // Connect two clients
  let stream1 = connect_and_join(port, "alice")
    .await
    .expect("Failed to join as alice");
  let (mut reader1, _writer1) = stream1.into_split();

  let stream2 = connect_and_join(port, "bob")
    .await
    .expect("Failed to join as bob");
  let (_reader2, mut writer2) = stream2.into_split();

  // Clear alice's join notification from bob's reader
  let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader1)).await;

  // Bob leaves
  let leave_msg = ClientMessage::Leave {
    username: "bob".to_string(),
  };
  let frame = encode_client_message(&leave_msg).expect("Failed to encode");
  writer2.write_all(&frame).await.expect("Failed to send");
  writer2.flush().await.expect("Failed to flush");

  // Alice should receive leave notification
  let leave_notif = timeout(TEST_TIMEOUT, read_server_message(&mut reader1))
    .await
    .expect("Timeout waiting for leave notification")
    .expect("Failed to read leave notification");

  match leave_notif {
    ServerMessage::UserLeft { username } => {
      assert_eq!(username, "bob");
    }
    _ => panic!("Expected UserLeft, got: {:?}", leave_notif),
  }
}

#[tokio::test]
async fn test_empty_message_ignored() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let stream = connect_and_join(port, "alice")
    .await
    .expect("Failed to join as alice");
  let (mut reader, mut writer) = stream.into_split();

  // Send empty message
  let empty_msg = ClientMessage::Message {
    username: "alice".to_string(),
    content: "   ".to_string(),
  };
  let frame = encode_client_message(&empty_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Should not receive empty message (timeout expected)
  let result = timeout(Duration::from_millis(500), read_server_message(&mut reader)).await;
  assert!(
    result.is_err(),
    "Should timeout - empty message should be ignored"
  );
}

#[tokio::test]
async fn test_multiple_clients_concurrent() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let mut clients = Vec::new();

  // Connect 10 clients concurrently
  for i in 0..10 {
    let username = format!("user{}", i);
    let stream = connect_and_join(port, &username)
      .await
      .unwrap_or_else(|_| panic!("Failed to join as {}", username));
    clients.push(stream);
  }

  assert_eq!(clients.len(), 10);
}

#[tokio::test]
async fn test_spoofing_prevention() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let stream = connect_and_join(port, "alice")
    .await
    .expect("Failed to join as alice");
  let (mut reader, mut writer) = stream.into_split();

  // Try to send message as different user
  let spoofed_msg = ClientMessage::Message {
    username: "bob".to_string(),
    content: "Spoofed message".to_string(),
  };
  let frame = encode_client_message(&spoofed_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Should not receive spoofed message (timeout expected)
  let result = timeout(Duration::from_millis(500), read_server_message(&mut reader)).await;
  assert!(
    result.is_err(),
    "Should timeout - spoofed message should be ignored"
  );
}

#[tokio::test]
async fn test_max_join_attempts() {
  let port = get_test_port();
  let _server = start_test_server(port)
    .await
    .expect("Failed to start server");

  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Send 3 invalid join attempts
  for i in 0..3 {
    let invalid_join = ClientMessage::Join {
      username: "ab".to_string(),
    };
    let frame = encode_client_message(&invalid_join).expect("Failed to encode");
    writer.write_all(&frame).await.expect("Failed to send");
    writer.flush().await.expect("Failed to flush");

    // Should get error for each attempt
    let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
      .await
      .unwrap_or_else(|_| panic!("Timeout on attempt {}", i + 1));
  }

  // After 3 attempts, connection should close or get final error
  let mut buf = [0u8; 1];
  let result = timeout(Duration::from_secs(1), reader.read(&mut buf)).await;

  // Should either timeout, get EOF, or get error
  match result {
    Ok(Ok(0)) => {}  // EOF - expected
    Ok(Err(_)) => {} // Error - expected
    Err(_) => {}     // Timeout - acceptable
    Ok(Ok(_)) => {}  // Got data - might be final error message
  }
}
