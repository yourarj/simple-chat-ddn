use std::io;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ApplicationError {
  #[error("Error in encoding")]
  Encoding(#[from] bincode::error::EncodeError),
  #[error("Error in decoding")]
  Decoding(#[from] bincode::error::DecodeError),
  #[error("Error in decoding")]
  StreamWriteError(#[from] io::Error),
  #[error("Client stream closed")]
  ClientReadStreamClosed,
  #[error("Incomplete length prefix")]
  IncompleteLengthPrefix,
  #[error("Incomplete payload")]
  IncompletePyaload,
  #[error("username not found")]
  UsernameNotFound,
}
