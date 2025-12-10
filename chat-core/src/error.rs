use std::io;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ApplicationError>;

#[derive(Error, Debug)]
pub enum ApplicationError {
  #[error("IO error: {0}")]
  Io(#[from] io::Error),

  #[error("Bincode encoding error: {0}")]
  BincodeEncode(#[from] bincode::error::EncodeError),

  #[error("Bincode decoding error: {0}")]
  BincodeDecode(#[from] bincode::error::DecodeError),

  #[error("Message size {actual} exceeds maximum allowed size {max}")]
  MessageTooLarge { actual: usize, max: usize },

  #[error("Username '{0}' is invalid: {1}")]
  InvalidUsername(String, String),

  #[error("Username not found in session")]
  UsernameNotFound,

  #[error("Client read stream closed")]
  ClientReadStreamClosed,

  #[error("Broadcast channel error: {0}")]
  BroadcastError(String),

  #[error("Channel send error")]
  ChannelSendError,

  #[error("Invalid frame: {0}")]
  InvalidFrame(String),

  #[error("Connection limit reached")]
  ConnectionLimitReached,

  #[error("Shutdown in progress")]
  ShutdownInProgress,

  #[error("Configuration error: {0}")]
  ConfigError(String),

  #[error("Miscellaneous error: {0}")]
  Miscellaneous(String),
}

impl ApplicationError {
  pub fn message_too_large(actual: usize, max: usize) -> Self {
    Self::MessageTooLarge { actual, max }
  }

  pub fn invalid_username(username: String, reason: String) -> Self {
    Self::InvalidUsername(username, reason)
  }

  pub fn invalid_frame(reason: String) -> Self {
    Self::InvalidFrame(reason)
  }

  pub fn config_error(message: String) -> Self {
    Self::ConfigError(message)
  }
}

impl<T> From<tokio::sync::mpsc::error::SendError<T>> for ApplicationError {
  fn from(_: tokio::sync::mpsc::error::SendError<T>) -> Self {
    Self::ChannelSendError
  }
}

impl From<tokio::sync::broadcast::error::SendError<crate::protocol::SharedServerMessage>>
  for ApplicationError
{
  fn from(
    e: tokio::sync::broadcast::error::SendError<crate::protocol::SharedServerMessage>,
  ) -> Self {
    Self::BroadcastError(format!("Send error: {}", e))
  }
}
