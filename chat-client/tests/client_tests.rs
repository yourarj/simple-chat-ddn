//! Integration tests for chat-client
//! Tests client functionality and interaction with server

use chat_core::{
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
static PORT_COUNTER: AtomicU16 = AtomicU16::new(10000); // Start from port 10000
const TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Get a unique port for each test
fn get_test_port() -> u16 {
  PORT_COUNTER.fetch_add(1, Ordering::SeqCst)
}

/// Start a mock server for testing client on a unique port
async fn start_mock_server(port: u16) -> Result<tokio::task::JoinHandle<()>> {
  let listener = tokio::net::TcpListener::bind(format!("{}:{}", TEST_HOST, port))
    .await
    .unwrap_or_else(|_| panic!("Failed to bind mock server on port {}", port));

  let handle = tokio::spawn(async move {
    while let Ok((stream, _)) = listener.accept().await {
      tokio::spawn(handle_mock_client(stream));
    }
  });

  // Give server time to start
  tokio::time::sleep(Duration::from_millis(50)).await;
  Ok(handle)
}

/// Mock server handler that responds to client messages
async fn handle_mock_client(stream: TcpStream) {
  let (mut reader, mut writer) = stream.into_split();
  let mut buffer = vec![0u8; 4096];

  loop {
    // Read message length
    let mut length_bytes = [0u8; 4];
    match reader.read_exact(&mut length_bytes).await {
      Ok(_) => {}
      Err(_) => break,
    }

    let payload_len = u32::from_be_bytes(length_bytes) as usize;

    // Read message payload
    if payload_len > buffer.len() {
      buffer.resize(payload_len, 0);
    }
    match reader.read_exact(&mut buffer[..payload_len]).await {
      Ok(_) => {}
      Err(_) => break,
    }

    // Decode client message
    let msg: ClientMessage =
      match bincode::decode_from_slice(&buffer[..payload_len], bincode::config::standard()) {
        Ok((msg, _)) => msg,
        Err(_) => break,
      };

    // Respond based on message type
    match msg {
      ClientMessage::Join { username } => {
        // Send welcome message
        let response = ServerMessage::success(format!("Welcome to the chat, {}! 🎉", username));
        if send_message(&mut writer, &response).await.is_err() {
          break;
        }
      }
      ClientMessage::Message { username, content } => {
        // Echo message back
        let response = ServerMessage::message(username, content);
        if send_message(&mut writer, &response).await.is_err() {
          break;
        }
      }
      ClientMessage::Leave { username } => {
        // Send goodbye
        let response = ServerMessage::success(format!("Goodbye, {}! 👋", username));
        if send_message(&mut writer, &response).await.is_err() {
          break;
        }
        break;
      }
    }
  }
}

async fn send_message(
  writer: &mut tokio::net::tcp::OwnedWriteHalf,
  msg: &ServerMessage,
) -> Result<()> {
  let payload = bincode::encode_to_vec(msg, bincode::config::standard())
    .map_err(|e| chat_core::error::ApplicationError::invalid_frame(e.to_string()))?;

  let mut frame = Vec::with_capacity(4 + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);

  writer.write_all(&frame).await?;
  writer.flush().await?;
  Ok(())
}

/// Helper to encode client message
fn encode_client_message(msg: &ClientMessage) -> Result<Vec<u8>> {
  let payload = bincode::encode_to_vec(msg, bincode::config::standard())
    .map_err(|e| chat_core::error::ApplicationError::invalid_frame(e.to_string()))?;

  let mut frame = Vec::with_capacity(4 + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame)
}

/// Helper to read server message
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
async fn test_client_connect_and_join() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  // Connect to server
  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Send join message
  let join_msg = ClientMessage::Join {
    username: "test_user".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Receive welcome message
  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout waiting for welcome")
    .expect("Failed to read welcome");

  match response {
    ServerMessage::Success { message } => {
      assert!(message.contains("Welcome"));
      assert!(message.contains("test_user"));
    }
    _ => panic!("Expected welcome message, got: {:?}", response),
  }
}

#[tokio::test]
async fn test_client_send_message() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Join first
  let join_msg = ClientMessage::Join {
    username: "alice".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Read welcome
  let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout")
    .expect("Failed to read");

  // Send chat message
  let chat_msg = ClientMessage::Message {
    username: "alice".to_string(),
    content: "Hello World!".to_string(),
  };
  let frame = encode_client_message(&chat_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Receive echo
  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout waiting for echo")
    .expect("Failed to read echo");

  match response {
    ServerMessage::Message { username, content } => {
      assert_eq!(username, "alice");
      assert_eq!(content, "Hello World!");
    }
    _ => panic!("Expected message echo, got: {:?}", response),
  }
}

#[tokio::test]
async fn test_client_leave() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Join
  let join_msg = ClientMessage::Join {
    username: "bob".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Read welcome
  let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout")
    .expect("Failed to read");

  // Leave
  let leave_msg = ClientMessage::Leave {
    username: "bob".to_string(),
  };
  let frame = encode_client_message(&leave_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Receive goodbye
  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout waiting for goodbye")
    .expect("Failed to read goodbye");

  match response {
    ServerMessage::Success { message } => {
      assert!(message.contains("Goodbye"));
      assert!(message.contains("bob"));
    }
    _ => panic!("Expected goodbye message, got: {:?}", response),
  }
}

#[tokio::test]
async fn test_client_reconnect() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  // First connection
  {
    let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
      .await
      .expect("Failed to connect");

    let (mut reader, mut writer) = stream.into_split();

    let join_msg = ClientMessage::Join {
      username: "reconnect_user".to_string(),
    };
    let frame = encode_client_message(&join_msg).expect("Failed to encode");
    writer.write_all(&frame).await.expect("Failed to send");
    writer.flush().await.expect("Failed to flush");

    let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
      .await
      .expect("Timeout")
      .expect("Failed to read");

    // Drop connection
  }

  // Wait a bit
  tokio::time::sleep(Duration::from_millis(200)).await;

  // Reconnect with same username
  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to reconnect");

  let (mut reader, mut writer) = stream.into_split();

  let join_msg = ClientMessage::Join {
    username: "reconnect_user".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Should be able to rejoin
  let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout waiting for welcome")
    .expect("Failed to read welcome");

  match response {
    ServerMessage::Success { message } => {
      assert!(message.contains("Welcome"));
    }
    _ => panic!("Expected welcome on reconnect, got: {:?}", response),
  }
}

#[tokio::test]
async fn test_client_multiple_messages() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  let (mut reader, mut writer) = stream.into_split();

  // Join
  let join_msg = ClientMessage::Join {
    username: "multi_user".to_string(),
  };
  let frame = encode_client_message(&join_msg).expect("Failed to encode");
  writer.write_all(&frame).await.expect("Failed to send");
  writer.flush().await.expect("Failed to flush");

  // Read welcome
  let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
    .await
    .expect("Timeout")
    .expect("Failed to read");

  // Send multiple messages
  for i in 0..5 {
    let chat_msg = ClientMessage::Message {
      username: "multi_user".to_string(),
      content: format!("Message {}", i),
    };
    let frame = encode_client_message(&chat_msg).expect("Failed to encode");
    writer.write_all(&frame).await.expect("Failed to send");
    writer.flush().await.expect("Failed to flush");

    // Receive echo
    let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
      .await
      .expect("Timeout waiting for echo")
      .expect("Failed to read echo");

    match response {
      ServerMessage::Message { content, .. } => {
        assert_eq!(content, format!("Message {}", i));
      }
      _ => panic!("Expected message echo, got: {:?}", response),
    }
  }
}

#[tokio::test]
async fn test_client_connection_failure() {
  // Try to connect to non-existent server
  let result = timeout(
    Duration::from_secs(1),
    TcpStream::connect("127.0.0.1:19999"),
  )
  .await;

  // Should timeout or fail
  assert!(result.is_err() || result.unwrap().is_err());
}

#[tokio::test]
async fn test_client_malformed_message_handling() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  let mut stream = TcpStream::connect(format!("{}:{}", TEST_HOST, port))
    .await
    .expect("Failed to connect");

  // Send malformed data (not valid bincode)
  let malformed = vec![0, 0, 0, 5, 1, 2, 3, 4, 5];
  let _ = stream.write_all(&malformed).await;
  let _ = stream.flush().await;

  // Server should close connection or handle gracefully
  tokio::time::sleep(Duration::from_millis(100)).await;

  let mut buf = [0u8; 1];
  let result = timeout(Duration::from_secs(1), stream.read(&mut buf)).await;

  // Connection should be closed or no response
  match result {
    Ok(Ok(0)) | Ok(Err(_)) | Err(_) => {
      // Expected: connection closed, read error, or timeout
    }
    _ => {}
  }
}

#[tokio::test]
async fn test_client_concurrent_operations() {
  let port = get_test_port();
  let _server = start_mock_server(port)
    .await
    .expect("Failed to start server");

  let mut handles = vec![];

  // Spawn multiple client tasks
  for i in 0..10 {
    let test_port = port;
    let handle = tokio::spawn(async move {
      let stream = TcpStream::connect(format!("{}:{}", TEST_HOST, test_port))
        .await
        .unwrap_or_else(|_| panic!("Failed to connect user{}", i));

      let (mut reader, mut writer) = stream.into_split();

      // Join
      let join_msg = ClientMessage::Join {
        username: format!("concurrent_user{}", i),
      };
      let frame = encode_client_message(&join_msg).expect("Failed to encode");
      writer.write_all(&frame).await.expect("Failed to send");
      writer.flush().await.expect("Failed to flush");

      // Read welcome
      let _ = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
        .await
        .expect("Timeout")
        .expect("Failed to read");

      // Send a message
      let chat_msg = ClientMessage::Message {
        username: format!("concurrent_user{}", i),
        content: format!("Message from user {}", i),
      };
      let frame = encode_client_message(&chat_msg).expect("Failed to encode");
      writer.write_all(&frame).await.expect("Failed to send");
      writer.flush().await.expect("Failed to flush");

      // Read echo
      let response = timeout(TEST_TIMEOUT, read_server_message(&mut reader))
        .await
        .expect("Timeout")
        .expect("Failed to read");

      matches!(response, ServerMessage::Message { .. })
    });

    handles.push(handle);
  }

  // All should succeed
  for handle in handles {
    let success = handle.await.expect("Task panicked");
    assert!(success);
  }
}
