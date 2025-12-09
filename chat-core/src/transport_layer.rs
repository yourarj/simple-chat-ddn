use bytes::{BufMut, Bytes, BytesMut};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tracing::{debug, warn};

use crate::error::{ApplicationError, Result};
use crate::message_cache::MessageCache;
use crate::protocol::{
  ClientMessage, LENGTH_PREFIX, MAX_MESSAGE_SIZE, ServerMessage, SharedServerMessage,
  decode_message, encode_message, encode_message_with_cache,
};

const WRITE_BUFFER_CAPACITY: usize = 64 * 1024;
const MAX_WRITE_BUFFER_SIZE: usize = 32 * 1024;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 10;

/// Read a single message from stream
pub async fn read_message_from_stream(
  reader: &mut OwnedReadHalf,
  buffer: &mut Vec<u8>,
) -> Result<ClientMessage> {
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
  buffer.clear();
  buffer.resize(payload_len, 0);
  reader.read_exact(buffer).await?;

  decode_message(buffer)
}

/// Write message to stream (single message)
pub async fn write_message_to_stream(
  writer: &mut OwnedWriteHalf,
  message: &ServerMessage,
) -> Result<()> {
  let frame = encode_message(message)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;
  Ok(())
}

/// Write message with cache support
pub async fn write_message_to_stream_with_cache(
  writer: &mut OwnedWriteHalf,
  message: &SharedServerMessage,
  cache: &MessageCache,
) -> Result<()> {
  let frame = encode_message_with_cache(message, cache)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;
  Ok(())
}

/// Batched write handler for efficient message forwarding
pub struct BatchedWriter {
  writer: OwnedWriteHalf,
  buffer: BytesMut,
  max_buffer_size: usize,
  flush_interval: Duration,
}

impl BatchedWriter {
  pub fn new(writer: OwnedWriteHalf) -> Self {
    Self::with_config(writer, MAX_WRITE_BUFFER_SIZE, DEFAULT_FLUSH_INTERVAL_MS)
  }

  pub fn with_config(
    writer: OwnedWriteHalf,
    max_buffer_size: usize,
    flush_interval_ms: u64,
  ) -> Self {
    Self {
      writer,
      buffer: BytesMut::with_capacity(WRITE_BUFFER_CAPACITY),
      max_buffer_size,
      flush_interval: Duration::from_millis(flush_interval_ms),
    }
  }

  /// Add message frame to buffer
  pub fn add_message(&mut self, frame: &Bytes) {
    self.buffer.put_slice(frame);
  }

  /// Check if buffer should be flushed based on size
  pub fn should_flush(&self) -> bool {
    self.buffer.len() >= self.max_buffer_size
  }

  /// Flush buffered messages to writer
  pub async fn flush(&mut self) -> Result<()> {
    if self.buffer.is_empty() {
      return Ok(());
    }

    debug!("Flushing {} bytes", self.buffer.len());

    self.writer.write_all(&self.buffer).await?;
    self.writer.flush().await?;
    self.buffer.clear();

    Ok(())
  }

  /// Get flush interval duration
  pub fn flush_interval(&self) -> Duration {
    self.flush_interval
  }

  /// Gracefully shutdown writer
  pub async fn shutdown(mut self) -> Result<()> {
    if let Err(e) = self.flush().await {
      warn!("Error during shutdown flush: {}", e);
    }

    if let Err(e) = self.writer.shutdown().await {
      warn!("Error shutting down writer: {}", e);
      return Err(e.into());
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_buffer_size_checks() {
    let buffer = BytesMut::with_capacity(WRITE_BUFFER_CAPACITY);
    assert!(buffer.capacity() >= MAX_WRITE_BUFFER_SIZE);
  }
}
