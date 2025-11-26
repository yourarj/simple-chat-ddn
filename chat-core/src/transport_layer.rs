use bincode::{Decode, Encode};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::tcp::OwnedReadHalf,
};

use crate::{
  error::ApplicationError,
  protocol::{LENGTH_PREFIX, decode_message, encode_message},
};

pub async fn read_message_from_stream<T: Decode<()>>(
  reader: &mut OwnedReadHalf,
  buffer: &mut Vec<u8>,
) -> Result<T, ApplicationError> {
  let n = reader.read(&mut buffer[..LENGTH_PREFIX]).await?;
  if n == 0 {
    return Err(ApplicationError::ClientReadStreamClosed);
  }

  if n < LENGTH_PREFIX {
    return Err(ApplicationError::IncompleteLengthPrefix);
  }

  let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

  if buffer.len() < length {
    buffer.resize(length, 0);
  }

  reader.read_exact(&mut buffer[..length]).await?;
  Ok(decode_message(&buffer[..length])?)
}

pub async fn write_message_to_stream<T: Encode>(
  writer: &mut tokio::net::tcp::OwnedWriteHalf,
  message: &T,
) -> Result<(), ApplicationError> {
  let frame = encode_message(message)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Arc;
  use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
  };

  #[tokio::test]
  async fn test_read_message_from_stream_success() {
    let test_message = crate::protocol::ClientMessage::Join {
      username: "testuser".to_string(),
    };

    let encoded_message =
      crate::protocol::encode_message(&test_message).expect("Failed to encode test message");

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (mut reader, mut writer) = stream.into_split();

      writer.write_all(&encoded_message).await.unwrap();
      writer.shutdown().await.unwrap();

      let mut buffer = vec![0u8; 1024];
      let _ = reader.read(&mut buffer).await.unwrap();

      Arc::new(test_message)
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let received_message: crate::protocol::ClientMessage =
        read_message_from_stream(&mut reader, &mut buffer)
          .await
          .expect("Failed to read message from stream");

      received_message
    });

    let (server_result, client_result) = tokio::join!(server_handle, client_handle);
    let sent_message = server_result.unwrap();
    let received_message = client_result.unwrap();

    match (&*sent_message, received_message) {
      (
        crate::protocol::ClientMessage::Join {
          username: sent_username,
        },
        crate::protocol::ClientMessage::Join {
          username: received_username,
        },
      ) => {
        assert_eq!(sent_username, &received_username);
      }
      _ => panic!("Message type mismatch"),
    }
  }

  #[tokio::test]
  async fn test_read_message_from_stream_client_closed() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      drop(stream);
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let result: Result<crate::protocol::ClientMessage, crate::error::ApplicationError> =
        read_message_from_stream(&mut reader, &mut buffer).await;

      result
    });

    server_handle.await.unwrap();
    let client_result = client_handle.await.unwrap();

    assert!(matches!(
      client_result,
      Err(crate::error::ApplicationError::ClientReadStreamClosed)
    ));
  }

  #[tokio::test]
  async fn test_read_message_from_stream_incomplete_length_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (_, mut writer) = stream.into_split();

      writer.write_all(&[0x00, 0x00]).await.unwrap();
      writer.shutdown().await.unwrap();
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let result: Result<crate::protocol::ClientMessage, crate::error::ApplicationError> =
        read_message_from_stream(&mut reader, &mut buffer).await;

      result
    });

    server_handle.await.unwrap();
    let client_result = client_handle.await.unwrap();

    assert!(matches!(
      client_result,
      Err(crate::error::ApplicationError::IncompleteLengthPrefix)
    ));
  }

  #[tokio::test]
  async fn test_read_message_from_stream_incomplete_payload() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (_, mut writer) = stream.into_split();

      writer.write_all(&[0x00, 0x00, 0x00, 0x64]).await.unwrap();
      writer.write_all(&[0xFF; 10]).await.unwrap();
      writer.shutdown().await.unwrap();
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let result: Result<crate::protocol::ClientMessage, crate::error::ApplicationError> =
        read_message_from_stream(&mut reader, &mut buffer).await;

      result
    });

    server_handle.await.unwrap();
    let client_result = client_handle.await.unwrap();

    assert!(client_result.is_err());
  }

  #[tokio::test]
  async fn test_write_message_to_stream_success() {
    let test_message = crate::protocol::ServerMessage::Success {
      message: "Test message".to_string(),
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (_, mut writer) = stream.into_split();

      write_message_to_stream(&mut writer, &test_message)
        .await
        .expect("Failed to write message to stream");

      writer.shutdown().await.unwrap();
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let bytes_read = reader.read(&mut buffer).await.unwrap();
      buffer.truncate(bytes_read);

      buffer
    });

    server_handle.await.unwrap();
    let received_bytes = client_handle.await.unwrap();

    let expected_message = crate::protocol::ServerMessage::Success {
      message: "Test message".to_string(),
    };
    let expected_bytes =
      crate::protocol::encode_message(&expected_message).expect("Failed to encode test message");

    assert_eq!(received_bytes, expected_bytes);
  }

  #[tokio::test]
  async fn test_write_message_to_stream_with_large_message() {
    let large_content = "x".repeat(10000);
    let test_message = crate::protocol::ServerMessage::Message {
      username: "largeuser".to_string(),
      content: large_content,
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (_, mut writer) = stream.into_split();

      write_message_to_stream(&mut writer, &test_message)
        .await
        .expect("Failed to write large message to stream");

      writer.shutdown().await.unwrap();
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 20480];
      let mut total_bytes = Vec::new();
      let mut offset = 0;

      loop {
        let bytes_read = reader.read(&mut buffer[offset..]).await.unwrap();
        if bytes_read == 0 {
          break;
        }
        offset += bytes_read;
        total_bytes.extend_from_slice(&buffer[..bytes_read]);
      }

      total_bytes
    });

    server_handle.await.unwrap();
    let received_bytes = client_handle.await.unwrap();

    let expected_large_content = "x".repeat(10000);
    let expected_message = crate::protocol::ServerMessage::Message {
      username: "largeuser".to_string(),
      content: expected_large_content,
    };
    let expected_bytes = crate::protocol::encode_message(&expected_message)
      .expect("Failed to encode large test message");

    assert_eq!(received_bytes, expected_bytes);
    assert!(received_bytes.len() > 10000);
  }

  #[tokio::test]
  async fn test_write_message_to_stream_with_empty_message() {
    let test_message = crate::protocol::ServerMessage::Message {
      username: "".to_string(),
      content: "".to_string(),
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_handle = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let (_, mut writer) = stream.into_split();

      write_message_to_stream(&mut writer, &test_message)
        .await
        .expect("Failed to write empty message to stream");

      writer.shutdown().await.unwrap();
    });

    let client_handle = tokio::spawn(async move {
      let stream = TcpStream::connect(addr).await.unwrap();
      let (mut reader, _) = stream.into_split();

      let mut buffer = vec![0u8; 4096];
      let bytes_read = reader.read(&mut buffer).await.unwrap();
      buffer.truncate(bytes_read);

      buffer
    });

    server_handle.await.unwrap();
    let received_bytes = client_handle.await.unwrap();

    let expected_message = crate::protocol::ServerMessage::Message {
      username: "".to_string(),
      content: "".to_string(),
    };
    let expected_bytes = crate::protocol::encode_message(&expected_message)
      .expect("Failed to encode empty test message");

    assert_eq!(received_bytes, expected_bytes);

    assert!(received_bytes.len() >= 4);
  }

  #[tokio::test]
  async fn test_round_trip_encoding_decoding() {
    let test_messages = vec![
      crate::protocol::ClientMessage::Join {
        username: "user1".to_string(),
      },
      crate::protocol::ClientMessage::Leave {
        username: "user2".to_string(),
      },
      crate::protocol::ClientMessage::Message {
        username: "user3".to_string(),
        content: "Hello, world!".to_string(),
      },
    ];

    for (i, original_message) in test_messages.into_iter().enumerate() {
      let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
      let addr = listener.local_addr().unwrap();

      let message_clone = original_message.clone();
      let server_handle = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (_, mut writer) = stream.into_split();

        write_message_to_stream(&mut writer, &message_clone)
          .await
          .expect("Failed to write message to stream");

        writer.shutdown().await.unwrap();
      });

      let client_handle = tokio::spawn(async move {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (mut reader, _) = stream.into_split();

        let mut buffer = vec![0u8; 4096];
        let received_message: crate::protocol::ClientMessage =
          read_message_from_stream(&mut reader, &mut buffer)
            .await
            .expect("Failed to read message from stream");

        received_message
      });

      server_handle.await.unwrap();
      let received_message = client_handle.await.unwrap();

      match (original_message, received_message) {
        (
          crate::protocol::ClientMessage::Join {
            username: orig_user,
          },
          crate::protocol::ClientMessage::Join {
            username: recv_user,
          },
        ) => {
          assert_eq!(
            *orig_user, recv_user,
            "Join message {} username mismatch",
            i
          );
        }
        (
          crate::protocol::ClientMessage::Leave {
            username: orig_user,
          },
          crate::protocol::ClientMessage::Leave {
            username: recv_user,
          },
        ) => {
          assert_eq!(
            *orig_user, recv_user,
            "Leave message {} username mismatch",
            i
          );
        }
        (
          crate::protocol::ClientMessage::Message {
            username: orig_user,
            content: orig_content,
          },
          crate::protocol::ClientMessage::Message {
            username: recv_user,
            content: recv_content,
          },
        ) => {
          assert_eq!(*orig_user, recv_user, "Message {} username mismatch", i);
          assert_eq!(
            *orig_content, recv_content,
            "Message {} content mismatch",
            i
          );
        }
        _ => panic!("Message {} type mismatch", i),
      }
    }
  }

  #[test]
  fn test_buffer_reuse() {
    let mut buffer = vec![0u8; 10];

    buffer[0] = 0x00;
    buffer[1] = 0x00;
    buffer[2] = 0x01;
    buffer[3] = 0x00;

    if buffer.len() < 256 {
      buffer.resize(256, 0);
    }

    assert!(
      buffer.len() >= 256,
      "Buffer should be resized to accommodate large messages"
    );

    buffer[0] = 0x42;
    let old_size = buffer.len();
    buffer.resize(old_size + 100, 0);

    assert_eq!(
      buffer[0], 0x42,
      "Buffer resize should preserve existing data"
    );
    assert_eq!(
      buffer.len(),
      old_size + 100,
      "Buffer should be resized correctly"
    );
  }
}
