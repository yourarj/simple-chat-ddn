//! Stress tests for the chat server
//!
//! Tests system behavior under high load

use chat_core::ApplicationError;
use std::sync::{
  Arc,
  atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};
use tokio::{io::AsyncWriteExt, time::timeout};

const STRESS_TEST_HOST: &str = "127.0.0.1";
const STRESS_TEST_PORT: u16 = 9998;
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::test]
#[ignore] // Run with: cargo test --test stress_test -- --ignored
async fn stress_test_many_clients() {
  // Start server
  let _server = start_stress_test_server().await;

  let num_clients = 100;
  let messages_per_client = 10;

  let start = Instant::now();
  let messages_sent = Arc::new(AtomicU64::new(0));
  let messages_received = Arc::new(AtomicU64::new(0));

  let mut handles = vec![];

  for i in 0..num_clients {
    let sent = Arc::clone(&messages_sent);
    let received = Arc::clone(&messages_received);

    let handle = tokio::spawn(async move {
      let username = format!("user{}", i);

      // Connect and join
      let result = timeout(TEST_TIMEOUT, connect_and_authenticate(&username)).await;

      if result.is_err() {
        eprintln!("Timeout connecting user{}", i);
        return;
      }

      let Ok(stream) = result.unwrap() else {
        eprintln!("Failed to connect user{}", i);
        return;
      };

      let (mut reader, mut writer) = stream.into_split();

      // Spawn reader task
      let recv = Arc::clone(&received);
      let reader_handle = tokio::spawn(async move {
        while (read_message_with_timeout(&mut reader, Duration::from_secs(5)).await).is_ok() {
          recv.fetch_add(1, Ordering::Relaxed);
        }
      });

      // Send messages
      for j in 0..messages_per_client {
        let msg = create_chat_message(&username, &format!("Message {} from {}", j, username));
        if writer.write_all(&msg).await.is_ok() {
          sent.fetch_add(1, Ordering::Relaxed);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }

      tokio::time::sleep(Duration::from_millis(500)).await;
      reader_handle.abort();
    });

    handles.push(handle);
  }

  // Wait for all clients to complete
  for handle in handles {
    let _ = handle.await;
  }

  let elapsed = start.elapsed();
  let sent_count = messages_sent.load(Ordering::Relaxed);
  let recv_count = messages_received.load(Ordering::Relaxed);

  println!("=== Stress Test Results ===");
  println!("Clients: {}", num_clients);
  println!("Messages per client: {}", messages_per_client);
  println!("Total messages sent: {}", sent_count);
  println!("Total messages received: {}", recv_count);
  println!("Duration: {:?}", elapsed);
  println!(
    "Throughput: {:.2} msg/sec",
    sent_count as f64 / elapsed.as_secs_f64()
  );

  // At least 90% of messages should be sent
  assert!(sent_count >= (num_clients * messages_per_client * 9 / 10) as u64);
}

#[tokio::test]
#[ignore]
async fn stress_test_rapid_connect_disconnect() {
  let _server = start_stress_test_server().await;

  let num_iterations = 50;
  let start = Instant::now();
  let mut successful_connections = 0;

  for i in 0..num_iterations {
    let username = format!("rapid_user{}", i);

    match timeout(Duration::from_secs(2), connect_and_authenticate(&username)).await {
      Ok(Ok(_)) => {
        successful_connections += 1;
        // Immediately disconnect (drop the stream)
      }
      _ => {
        eprintln!("Failed to connect iteration {}", i);
      }
    }

    tokio::time::sleep(Duration::from_millis(20)).await;
  }

  let elapsed = start.elapsed();

  println!("=== Rapid Connect/Disconnect Test ===");
  println!("Iterations: {}", num_iterations);
  println!("Successful: {}", successful_connections);
  println!("Duration: {:?}", elapsed);
  println!(
    "Rate: {:.2} conn/sec",
    successful_connections as f64 / elapsed.as_secs_f64()
  );

  assert!(successful_connections >= num_iterations * 9 / 10);
}

// Helper functions

async fn start_stress_test_server() -> tokio::task::JoinHandle<()> {
  use chat_core::message_cache::MessageCache;
  use std::sync::Arc;
  use tokio::sync::broadcast;

  let handle = tokio::spawn(async move {
    let (tx, _rx) = broadcast::channel(10000);
    let broadcaster = Arc::new(tx);

    let listener =
      tokio::net::TcpListener::bind(format!("{}:{}", STRESS_TEST_HOST, STRESS_TEST_PORT))
        .await
        .expect("Failed to bind stress test server");

    let broadcast_pool = chat_server::broadcast_pool::BroadcastPool::new(broadcaster);
    let cache = MessageCache::new(10000);

    while let Ok((stream, _)) = listener.accept().await {
      let pool = broadcast_pool.clone();
      let cache_clone = cache.clone();
      tokio::spawn(async move {
        let _ = chat_server::client_handler::handle_client(stream, pool, cache_clone).await;
      });
    }
  });

  tokio::time::sleep(Duration::from_millis(100)).await;
  handle
}

async fn connect_and_authenticate(
  username: &str,
) -> chat_core::error::Result<tokio::net::TcpStream> {
  use chat_core::protocol::ClientMessage;
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  let stream =
    tokio::net::TcpStream::connect(format!("{}:{}", STRESS_TEST_HOST, STRESS_TEST_PORT)).await?;
  let (mut reader, mut writer) = stream.into_split();

  // Send join
  let join_msg = ClientMessage::Join {
    username: username.to_string(),
  };
  let frame = encode_message(&join_msg)?;
  writer.write_all(&frame).await?;

  // Read welcome (with timeout)
  let mut length_bytes = [0u8; 4];
  reader.read_exact(&mut length_bytes).await?;
  let payload_len = u32::from_be_bytes(length_bytes) as usize;
  let mut payload = vec![0u8; payload_len];
  reader.read_exact(&mut payload).await?;

  reader
    .reunite(writer)
    .map_err(|_| ApplicationError::Miscellaneous(String::from("Reader Writer reunite issue")))
}

fn encode_message(msg: &chat_core::protocol::ClientMessage) -> chat_core::error::Result<Vec<u8>> {
  let payload = bincode::encode_to_vec(msg, bincode::config::standard())
    .map_err(|e| chat_core::error::ApplicationError::invalid_frame(e.to_string()))?;

  let mut frame = Vec::with_capacity(4 + payload.len());
  frame.extend_from_slice(&(payload.len() as u32).to_be_bytes());
  frame.extend_from_slice(&payload);
  Ok(frame)
}

fn create_chat_message(username: &str, content: &str) -> Vec<u8> {
  use chat_core::protocol::ClientMessage;

  let msg = ClientMessage::Message {
    username: username.to_string(),
    content: content.to_string(),
  };
  encode_message(&msg).unwrap()
}

async fn read_message_with_timeout(
  reader: &mut tokio::net::tcp::OwnedReadHalf,
  duration: Duration,
) -> chat_core::error::Result<()> {
  use tokio::io::AsyncReadExt;

  let read_op = async {
    let mut length_bytes = [0u8; 4];
    reader.read_exact(&mut length_bytes).await?;
    let payload_len = u32::from_be_bytes(length_bytes) as usize;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await?;
    Ok::<(), chat_core::error::ApplicationError>(())
  };

  timeout(duration, read_op)
    .await
    .map_err(|_| chat_core::error::ApplicationError::config_error("Timeout".to_string()))?
}
