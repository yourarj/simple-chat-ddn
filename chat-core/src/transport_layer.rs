use bincode::{Decode, Encode};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::tcp::OwnedReadHalf,
};

use crate::{
  error::ApplicationError,
  protocol::{LENGTH_PREFIX, decode_message, encode_message},
};

/// Read a server message
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

/// Write a client-to-server message
pub async fn write_message_to_stream<T: Encode>(
  writer: &mut tokio::net::tcp::OwnedWriteHalf,
  message: &T,
) -> Result<(), ApplicationError> {
  let frame = encode_message(message)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;
  Ok(())
}
