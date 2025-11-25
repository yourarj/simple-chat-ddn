use tokio::{io::AsyncReadExt, net::tcp::OwnedReadHalf};

use crate::{
  error::ApplicationError,
  protocol::{ChatMessage, LENGTH_PREFIX, decode_message},
};

pub async fn read_next_message_from_stream(
  reader: &mut OwnedReadHalf,
  buffer: &mut Vec<u8>,
) -> Result<ChatMessage, ApplicationError> {
  // Read length prefix

  let n = reader.read(&mut buffer[..LENGTH_PREFIX]).await?;
  if n == 0 {
    return Err(ApplicationError::ClientReadStreamClosed); // Connection closed
  }

  if n < LENGTH_PREFIX {
    return Err(ApplicationError::IncompleteLengthPrefix);
  }

  let length = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;

  // Ensure buffer is large enough
  if buffer.len() < length {
    buffer.resize(length, 0);
  }

  // Read message payload
  let n = reader.read_exact(&mut buffer[..length]).await?;
  if n < length {
    return Err(ApplicationError::IncompletePyaload);
  }

  Ok(decode_message(&buffer[..length])?)
}
