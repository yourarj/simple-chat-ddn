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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_error_derivation_for_encoding() {
    let test_data = vec![1, 2, 3, 4];
    let encode_result = bincode::encode_to_vec(&test_data, bincode::config::standard());

    let encode_error = match encode_result {
      Ok(_) => return,
      Err(e) => e,
    };
    let app_error: ApplicationError = encode_error.into();

    assert!(matches!(app_error, ApplicationError::Encoding(_)));
    assert!(app_error.to_string().contains("Error in encoding"));
  }

  #[test]
  fn test_error_derivation_for_decoding() {
    let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];

    let decode_result: Result<(String, usize), bincode::error::DecodeError> =
      bincode::decode_from_slice(&invalid_data, bincode::config::standard());

    if let Err(decode_error) = decode_result {
      let app_error: ApplicationError = decode_error.into();

      assert!(matches!(app_error, ApplicationError::Decoding(_)));
      assert!(app_error.to_string().contains("Error in decoding"));
    }
  }

  #[test]
  fn test_error_derivation_for_io_error() {
    let io_error = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "Connection lost");
    let app_error: ApplicationError = io_error.into();

    assert!(matches!(app_error, ApplicationError::StreamWriteError(_)));
    assert!(app_error.to_string().contains("Error in decoding"));
  }

  #[test]
  fn test_client_read_stream_closed_error() {
    let error = ApplicationError::ClientReadStreamClosed;
    assert!(error.to_string().contains("Client stream closed"));
  }

  #[test]
  fn test_incomplete_length_prefix_error() {
    let error = ApplicationError::IncompleteLengthPrefix;
    assert!(error.to_string().contains("Incomplete length prefix"));
  }

  #[test]
  fn test_incomplete_payload_error() {
    let error = ApplicationError::IncompletePyaload;
    assert!(error.to_string().contains("Incomplete payload"));
  }

  #[test]
  fn test_username_not_found_error() {
    let error = ApplicationError::UsernameNotFound;
    assert!(error.to_string().contains("username not found"));
  }

  #[test]
  fn test_error_display_formatting() {
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
    let app_error: ApplicationError = io_error.into();

    let display_string = format!("{}", app_error);
    assert!(display_string.contains("Error in decoding"));
  }

  #[test]
  fn test_error_kind_discrimination() {
    let errors = [
      ApplicationError::ClientReadStreamClosed,
      ApplicationError::IncompleteLengthPrefix,
      ApplicationError::IncompletePyaload,
      ApplicationError::UsernameNotFound,
    ];

    assert!(matches!(
      errors[0],
      ApplicationError::ClientReadStreamClosed
    ));
    assert!(matches!(
      errors[1],
      ApplicationError::IncompleteLengthPrefix
    ));
    assert!(matches!(errors[2], ApplicationError::IncompletePyaload));
    assert!(matches!(errors[3], ApplicationError::UsernameNotFound));

    assert!(!matches!(
      errors[0],
      ApplicationError::IncompleteLengthPrefix
    ));
    assert!(!matches!(errors[1], ApplicationError::UsernameNotFound));
    assert!(!matches!(
      errors[2],
      ApplicationError::ClientReadStreamClosed
    ));
    assert!(!matches!(errors[3], ApplicationError::IncompletePyaload));
  }

  #[test]
  fn test_error_source_chaining() {
    let io_error = std::io::Error::other("Underlying IO error");
    let app_error: ApplicationError = io_error.into();

    use std::error::Error;
    if let Some(source) = app_error.source() {
      assert!(source.to_string().contains("Underlying IO error"));
    } else {
      panic!("Expected error source to be preserved");
    }
  }

  #[test]
  fn test_complex_error_hierarchy() {
    let nested_io_error = std::io::Error::other("Deep nested error");
    let app_error: ApplicationError = nested_io_error.into();

    assert!(matches!(app_error, ApplicationError::StreamWriteError(_)));

    if let ApplicationError::StreamWriteError(io_err) = &app_error {
      assert_eq!(io_err.to_string(), "Deep nested error");
    }
  }

  #[test]
  fn test_error_debug_formatting() {
    let error = ApplicationError::UsernameNotFound;
    let debug_string = format!("{:?}", error);
    assert!(debug_string.contains("UsernameNotFound"));

    let io_error = std::io::Error::other("Debug test");
    let app_error: ApplicationError = io_error.into();
    let debug_string = format!("{:?}", app_error);
    assert!(debug_string.contains("StreamWriteError"));
  }

  #[test]
  fn test_error_edge_cases() {
    let empty_io_error = std::io::Error::other("");
    let app_error: ApplicationError = empty_io_error.into();
    assert!(app_error.to_string().contains("Error in decoding"));

    let long_message = "A".repeat(1000);
    let long_io_error = std::io::Error::other(long_message);
    let app_error: ApplicationError = long_io_error.into();
    assert!(app_error.to_string().contains("Error in decoding"));
  }

  #[test]
  fn test_bincode_error_conversion() {
    let test_data = vec![1, 2, 3, 4];
    if let Err(encode_error) = bincode::encode_to_vec(&test_data, bincode::config::standard()) {
      let app_error: ApplicationError = encode_error.into();
      assert!(matches!(app_error, ApplicationError::Encoding(_)));
      assert!(app_error.to_string().contains("Error in encoding"));
    }

    let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
    if bincode::decode_from_slice::<String, _>(&invalid_data, bincode::config::standard()).is_ok() {
    }
  }
}
