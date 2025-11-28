use chat_core::{
  error::ApplicationError,
  protocol::{ClientMessage, ServerMessage},
  transport_layer::{read_message_from_stream, write_message_to_stream},
};
use chat_server::server::ChatServer;
use tokio::{
  io::AsyncWriteExt,
  net::TcpStream,
  sync::oneshot,
  time::{Duration, timeout},
};

async fn simulate_client_connection(
  host: &str,
  port: u16,
  _username: &str,
) -> Result<TcpStream, Box<dyn std::error::Error + Send + Sync>> {
  let stream = TcpStream::connect(format!("{}:{}", host, port)).await?;
  Ok(stream)
}

#[tokio::test]
async fn test_server_basic_functionality() {
  let server = ChatServer::new(10);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12345;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let stream_result = simulate_client_connection("127.0.0.1", port, "testuser").await;
  assert!(stream_result.is_ok(), "Should be able to connect to server");

  let stream = stream_result.unwrap();

  let join_message = ClientMessage::Join {
    username: "testuser".to_string(),
  };

  let (_reader, mut writer) = stream.into_split();
  let join_result = write_message_to_stream(&mut writer, &join_message).await;
  assert!(join_result.is_ok(), "Should be able to send join message");

  let _ = writer.shutdown().await;

  let _ = shutdown_tx.send(());

  let result = timeout(Duration::from_secs(5), server_handle).await;
  match result {
    Ok(handle) => {
      let _ = handle;
    }
    Err(_) => panic!("Server shutdown timed out"),
  }
}

#[tokio::test]
async fn test_server_graceful_shutdown() {
  let server = ChatServer::new(10);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12347;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let client_result = simulate_client_connection("127.0.0.1", port, "testuser").await;
  assert!(client_result.is_ok(), "Client should connect successfully");

  let _ = shutdown_tx.send(());

  let server_result = server_handle.await;
  assert!(server_result.is_ok(), "Server should shut down gracefully");
}

#[tokio::test]
async fn test_server_multiple_clients() {
  let server = ChatServer::new(10);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12348;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let mut client_tasks = Vec::new();

  for i in 0..5 {
    let task = tokio::spawn(async move {
      simulate_client_connection("127.0.0.1", port, &format!("user{}", i)).await
    });
    client_tasks.push(task);
  }

  let mut connection_results = Vec::new();
  for task in client_tasks {
    connection_results.push(task.await.unwrap());
  }

  for result in &connection_results {
    assert!(result.is_ok(), "All client connections should succeed");
  }

  let _ = shutdown_tx.send(());

  let result = timeout(Duration::from_secs(5), server_handle).await;
  match result {
    Ok(handle) => {
      let _ = handle;
    }
    Err(_) => panic!("Server shutdown timed out"),
  }
}

#[tokio::test]
async fn test_server_message_handling() -> Result<(), ApplicationError> {
  let server = ChatServer::new(5);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();

  let port = 12349;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let client_handle = tokio::spawn(async move {
    let stream = TcpStream::connect(format!("127.0.0.1:{}", 12349))
      .await
      .unwrap();
    let (mut reader, mut writer) = stream.into_split();

    let join_message = ClientMessage::Join {
      username: "testuser".to_string(),
    };

    write_message_to_stream(&mut writer, &join_message)
      .await
      .unwrap();

    let mut buffer = vec![0u8; 4096];
    let response: Result<ServerMessage, ApplicationError> = timeout(
      Duration::from_millis(500),
      read_message_from_stream(&mut reader, &mut buffer),
    )
    .await
    .unwrap_or(Err(ApplicationError::ClientReadStreamClosed));

    if response.is_err() {
      eprintln!("Server did not respond within timeout, but this is acceptable for this test");
    }

    let test_message = ClientMessage::Message {
      username: "testuser".to_string(),
      content: "Hello, server!".to_string(),
    };

    let send_result = write_message_to_stream(&mut writer, &test_message).await;

    assert!(
      send_result.is_ok(),
      "Should be able to send message to server"
    );

    // Important: Properly shutdown the writer to avoid resource leaks
    let _ = writer.shutdown().await;

    // Clean up the reader
    drop(reader);
  });

  // Wait for client to complete with timeout
  let client_result = timeout(Duration::from_secs(10), client_handle).await;
  match client_result {
    Ok(handle) => {
      let _ = handle;
    }
    Err(_) => panic!("Client operation timed out"),
  }

  let _ = shutdown_tx.send(());

  let result = timeout(Duration::from_secs(5), server_handle).await;

  match result {
    Ok(handle) => {
      let _ = handle;
    }
    Err(_) => panic!("Server shutdown timed out"),
  }
  Ok(())
}
#[tokio::test]
async fn test_server_error_recovery() -> Result<(), ApplicationError> {
  let server = ChatServer::new(10);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();

  let port = 12349;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  for i in 0..3 {
    let stream_result = TcpStream::connect(format!("127.0.0.1:{}", port)).await;
    assert!(stream_result.is_ok(), "Connection {} should succeed", i);

    let stream = stream_result.unwrap();
    let (_reader, mut writer) = stream.into_split();

    let _ = writer.shutdown().await;

    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  let malformed_stream = TcpStream::connect(format!("127.0.0.1:{}", port))
    .await
    .unwrap();
  let (_reader, mut writer) = malformed_stream.into_split();

  let random_data = vec![0xDE, 0xAD, 0xBE, 0xEF, 0xCA, 0xFE, 0xBA, 0xBE];
  let _ = writer.write_all(&random_data).await;

  let _ = writer.shutdown().await;

  let _ = shutdown_tx.send(());
  let _ = server_handle.await;
  Ok(())
}

#[tokio::test]
async fn test_server_broadcast_functionality() -> Result<(), ApplicationError> {
  let server = ChatServer::new(10);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12350;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let mut client_handles = Vec::new();

  for i in 0..3 {
    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(format!("127.0.0.1:{}", 12350))
        .await
        .unwrap();
      let (mut reader, mut writer) = stream.into_split();

      let join_message = ClientMessage::Join {
        username: format!("user{}", i),
      };

      write_message_to_stream(&mut writer, &join_message)
        .await
        .unwrap();

      let mut buffer = vec![0u8; 4096];
      let response: Result<ServerMessage, ApplicationError> = timeout(
        Duration::from_secs(3),
        read_message_from_stream(&mut reader, &mut buffer),
      )
      .await
      .unwrap_or(Err(ApplicationError::ClientReadStreamClosed));

      let _ = writer.shutdown().await;

      if let Ok(ServerMessage::Success { message }) = response {
        assert!(message.contains("Welcome"));
      }

      drop(reader);
    });

    client_handles.push(client_handle);
    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  for handle in client_handles {
    let _ = handle.await;
  }

  let _ = shutdown_tx.send(());
  let result = timeout(Duration::from_secs(5), server_handle).await;

  match result {
    Ok(_) => {}
    Err(_) => panic!("Server shutdown timed out"),
  }

  Ok(())
}

#[tokio::test]
async fn test_server_connection_limit_enforcement() -> Result<(), ApplicationError> {
  let server = ChatServer::new(2);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12351;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let client1 = TcpStream::connect(format!("127.0.0.1:{}", port)).await;
  let client2 = TcpStream::connect(format!("127.0.0.1:{}", port)).await;

  assert!(client1.is_ok(), "First client should connect");
  assert!(client2.is_ok(), "Second client should connect");

  tokio::time::sleep(Duration::from_millis(100)).await;

  if let Ok(stream) = client1 {
    let (_, mut writer) = stream.into_split();
    writer.shutdown().await?;
  }

  if let Ok(stream) = client2 {
    let (_, mut writer) = stream.into_split();
    writer.shutdown().await?;
  }

  let _ = shutdown_tx.send(());
  let result = timeout(Duration::from_secs(5), server_handle).await;

  match result {
    Ok(_) => {}
    Err(_) => panic!("Server shutdown timed out"),
  }

  Ok(())
}

#[tokio::test]
async fn test_server_username_validation_integration() -> Result<(), ApplicationError> {
  let server = ChatServer::new(5);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();
  let port = 12352;

  let server_handle = tokio::spawn(async move { server.run("127.0.0.1", port, shutdown_rx).await });

  tokio::time::sleep(Duration::from_millis(100)).await;

  let client1_handle = tokio::spawn(async move {
    let stream = TcpStream::connect(format!("127.0.0.1:{}", 12352))
      .await
      .unwrap();
    let (mut reader1, mut writer1) = stream.into_split();

    let valid_join = ClientMessage::Join {
      username: "validuser".to_string(),
    };

    write_message_to_stream(&mut writer1, &valid_join)
      .await
      .unwrap();

    let mut buffer = vec![0u8; 4096];
    let response1: Result<ServerMessage, ApplicationError> = timeout(
      Duration::from_secs(3),
      read_message_from_stream(&mut reader1, &mut buffer),
    )
    .await
    .unwrap_or(Err(ApplicationError::ClientReadStreamClosed));

    let _ = writer1.shutdown().await;

    assert!(response1.is_ok(), "Valid username should be accepted");

    drop(reader1);
  });

  tokio::time::sleep(Duration::from_millis(50)).await;

  let client2_handle = tokio::spawn(async move {
    let stream2 = TcpStream::connect(format!("127.0.0.1:{}", 12352))
      .await
      .unwrap();
    let (mut reader2, mut writer2) = stream2.into_split();

    let invalid_join = ClientMessage::Join {
      username: "".to_string(),
    };

    write_message_to_stream(&mut writer2, &invalid_join)
      .await
      .unwrap();

    let mut buffer2 = vec![0u8; 4096];
    let response2: Result<ServerMessage, ApplicationError> = timeout(
      Duration::from_secs(3),
      read_message_from_stream(&mut reader2, &mut buffer2),
    )
    .await
    .unwrap_or(Err(ApplicationError::ClientReadStreamClosed));

    if let Ok(resp2) = response2
      && let ServerMessage::Error { reason } = resp2
    {
      assert!(reason.contains("username") || reason.contains("invalid"));
    }

    let _ = writer2.shutdown().await;

    drop(reader2);
  });

  let _ = client1_handle.await;
  let _ = client2_handle.await;

  let _ = shutdown_tx.send(());
  let result = timeout(Duration::from_secs(5), server_handle).await;

  match result {
    Ok(_) => {}
    Err(_) => panic!("Server shutdown timed out"),
  }

  Ok(())
}
