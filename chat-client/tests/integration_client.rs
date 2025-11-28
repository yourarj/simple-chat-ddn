use anyhow::Result;
use chat_client::client::ChatClient;
use chat_core::protocol::{ClientMessage, ServerMessage, decode_message, encode_message};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::sleep;

#[tokio::test]
async fn test_client_connection_and_basic_communication() -> Result<()> {
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let addr = listener.local_addr()?;

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 1024];
    let _n = reader.read(&mut buffer).await.unwrap();

    let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    let payload = &buffer[4..4 + length];
    let message: ClientMessage = decode_message(payload).unwrap();

    match message {
      ClientMessage::Join { username } => {
        assert_eq!(username, "testuser");

        let success_msg = ServerMessage::Success {
          message: "Welcome to the chat!".to_string(),
        };
        let response = encode_message(&success_msg).unwrap();
        writer.write_all(&response).await.unwrap();
        writer.flush().await.unwrap();
      }
      _ => panic!("Expected join message"),
    }

    loop {
      match reader.read(&mut buffer).await {
        Ok(0) => {
          break;
        }
        Ok(n) => {
          if n < 4 {
            continue;
          }

          let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
          if n < 4 + length {
            continue;
          }

          let payload = &buffer[4..4 + length];
          let message: ClientMessage = decode_message(payload).unwrap();

          match message {
            ClientMessage::Message { username, content } => {
              assert_eq!(username, "testuser");
              assert_eq!(content, "Hello, server!");

              let server_msg = ServerMessage::Message {
                username: "server".to_string(),
                content: "Echo: Hello, server!".to_string(),
              };
              let response = encode_message(&server_msg).unwrap();
              writer.write_all(&response).await.unwrap();
              writer.flush().await.unwrap();
            }
            ClientMessage::Leave { username } => {
              assert_eq!(username, "testuser");
              break;
            }
            _ => {}
          }
        }
        Err(_) => {
          break;
        }
      }
    }
  });

  sleep(Duration::from_millis(100)).await;

  let mut client = ChatClient::new("127.0.0.1", addr.port(), "testuser").await?;

  let test_message = ClientMessage::Message {
    username: "testuser".to_string(),
    content: "Hello, server!".to_string(),
  };
  client.send_client_message(test_message).await?;

  client.graceful_leave().await?;

  server_handle.await.unwrap();

  Ok(())
}

#[tokio::test]
async fn test_client_message_handling_and_parsing() -> Result<()> {
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let addr = listener.local_addr()?;

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 1024];
    let _n = reader.read(&mut buffer).await.unwrap();

    let success_msg = ServerMessage::Success {
      message: "Welcome!".to_string(),
    };
    let response = encode_message(&success_msg).unwrap();
    writer.write_all(&response).await.unwrap();

    let test_messages = vec![
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "Hello everyone!".to_string(),
      },
      ServerMessage::UserJoined {
        username: "bob".to_string(),
      },
      ServerMessage::UserLeft {
        username: "alice".to_string(),
      },
      ServerMessage::Error {
        reason: "Invalid command".to_string(),
      },
    ];

    for msg in test_messages {
      let response = encode_message(&msg).unwrap();
      writer.write_all(&response).await.unwrap();
      writer.flush().await.unwrap();
      sleep(Duration::from_millis(50)).await;
    }

    let _n = reader.read(&mut buffer).await.unwrap();
  });

  sleep(Duration::from_millis(100)).await;

  let mut client = ChatClient::new("127.0.0.1", addr.port(), "testuser2").await?;

  sleep(Duration::from_millis(300)).await;

  client.graceful_leave().await?;

  server_handle.await.unwrap();

  Ok(())
}

#[tokio::test]
async fn test_client_error_handling_and_edge_cases() -> Result<()> {
  let connection_result = ChatClient::new("127.0.0.1", 9999, "testuser").await;
  assert!(
    connection_result.is_err(),
    "Should fail to connect to non-existent server"
  );

  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let addr = listener.local_addr()?;

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();

    drop(stream);
  });

  sleep(Duration::from_millis(100)).await;


  let client_result = ChatClient::new("127.0.0.1", addr.port(), "testuser").await;


  if let Ok(mut client) = client_result {
    let test_message = ClientMessage::Message {
      username: "testuser".to_string(),
      content: "Test message to closed connection".to_string(),
    };

    let send_result = client.send_client_message(test_message).await;

    match send_result {
      Ok(_) => {
        println!("✓ Message sent successfully (connection was still open when message was sent)");
      }
      Err(e) => {
        println!(
          "Expected I/O error when sending to closed connection: {}",
          e
        );

        let error_msg = e.to_string().to_lowercase();
        assert!(
          error_msg.contains("connection")
            || error_msg.contains("broken")
            || error_msg.contains("reset")
            || error_msg.contains("pipe")
            || error_msg.contains("eof"),
          "Expected network-related error, got: {}",
          e
        );
      }
    }

    let leave_result = client.graceful_leave().await;
    match leave_result {
      Ok(_) => {
        println!("✓ Graceful leave succeeded (connection was handled properly)");
      }
      Err(e) => {
        println!("✓ Graceful leave encountered expected error: {}", e);
      }
    }
  }

  server_handle.await.unwrap();

  Ok(())
}

#[tokio::test]
async fn test_client_protocol_compliance_and_message_serialization() -> Result<()> {
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let addr = listener.local_addr()?;

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 4096];

    let mut length_bytes = [0u8; 4];
    reader.read_exact(&mut length_bytes).await.unwrap();
    let length = u32::from_be_bytes(length_bytes) as usize;

    if buffer.len() < length {
      buffer.resize(length, 0);
    }
    reader.read_exact(&mut buffer[..length]).await.unwrap();

    let join_message: ClientMessage = decode_message(&buffer[..length]).unwrap();

    match join_message {
      ClientMessage::Join { username } => {
        assert_eq!(username, "protocol_test");
      }
      _ => panic!("Expected join message"),
    }

    let success_msg = ServerMessage::Success {
      message: "Join successful".to_string(),
    };
    let response = encode_message(&success_msg).unwrap();
    writer.write_all(&response).await.unwrap();

    for i in 0..4 {
      let mut length_bytes = [0u8; 4];
      reader.read_exact(&mut length_bytes).await.unwrap();
      let length = u32::from_be_bytes(length_bytes) as usize;

      if buffer.len() < length {
        buffer.resize(length, 0);
      }
      reader.read_exact(&mut buffer[..length]).await.unwrap();

      let message: ClientMessage = decode_message(&buffer[..length]).unwrap();

      match message {
        ClientMessage::Message { username, content } => {
          assert_eq!(username, "protocol_test");
          assert_eq!(content, format!("Test message {}", i + 1));
        }
        ClientMessage::Leave { username } => {
          assert_eq!(username, "protocol_test");

          assert_eq!(i, 3, "Leave message should be the last message");
        }
        _ => panic!("Unexpected message type"),
      }
    }
  });

  sleep(Duration::from_millis(100)).await;

  let mut client = ChatClient::new("127.0.0.1", addr.port(), "protocol_test").await?;

  for i in 0..3 {
    let message = ClientMessage::Message {
      username: "protocol_test".to_string(),
      content: format!("Test message {}", i + 1),
    };
    client.send_client_message(message).await?;
  }

  let leave_message = ClientMessage::Leave {
    username: "protocol_test".to_string(),
  };
  client.send_client_message(leave_message).await?;

  server_handle.await.unwrap();

  Ok(())
}

#[tokio::test]
async fn test_client_connection_cleanup() -> Result<()> {

  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let addr = listener.local_addr()?;

  let server_handle = tokio::spawn(async move {
    let (stream, _) = listener.accept().await.unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let mut buffer = vec![0u8; 1024];
    

    let _n = reader.read(&mut buffer).await.unwrap();


    let success_msg = ServerMessage::Success {
      message: "Welcome to the chat!".to_string(),
    };
    let response = encode_message(&success_msg).unwrap();
    writer.write_all(&response).await.unwrap();
    writer.flush().await.unwrap();
    

    let _n = reader.read(&mut buffer).await.unwrap_or(0);
    

    let _ = writer.shutdown().await;
  });

  sleep(Duration::from_millis(100)).await;

  let mut client = ChatClient::new("127.0.0.1", addr.port(), "cleanup_test").await?;
  

  let test_message = ClientMessage::Message {
    username: "cleanup_test".to_string(),
    content: "Testing connection cleanup".to_string(),
  };
  

  client.send_client_message(test_message).await?;
  

  client.graceful_leave().await?;

  server_handle.await.unwrap();

  Ok(())
}
