use crate::{
  error::ApplicationError,
  protocol::{MAX_MESSAGE_SIZE, MAX_USERNAME_LENGTH},
};

pub fn generate_unique_id() -> String {
  let ts = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .expect("invalid time")
    .as_nanos();

  format!("id_{}", ts)
}

pub fn validate_username(username: &str) -> Result<(), ApplicationError> {
  if username.is_empty() || username.len() > MAX_USERNAME_LENGTH {
    return Err(ApplicationError::UsernameNotFound);
  }

  if username.len() > 20 {
    return Err(ApplicationError::message_too_large(
      username.len(),
      MAX_MESSAGE_SIZE,
    ));
  }
  Ok(())
}

pub fn is_valid_message(message: &str) -> bool {
  !message.is_empty()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_unique_id() {
    let id1 = generate_unique_id();
    let id2 = generate_unique_id();

    assert!(id1.starts_with("id_"));
    assert!(id2.starts_with("id_"));
    assert_ne!(id1, id2);
  }

  #[test]
  fn test_validate_username_valid() {
    let valid_usernames = vec!["alice", "user123", "test_user", "a"];

    for username in valid_usernames {
      assert!(validate_username(username).is_ok());
    }
  }

  #[test]
  fn test_validate_username_empty() {
    let result = validate_username("");
    assert!(result.is_err());
  }

  #[test]
  fn test_validate_username_too_long() {
    let long_username = "a".repeat(31);
    let result = validate_username(&long_username);
    assert!(result.is_err());
  }

  #[test]
  fn test_validate_username_over_20_chars() {
    let long_username = "a".repeat(21);
    let result = validate_username(&long_username);
    assert!(result.is_err());
  }

  #[test]
  fn test_is_valid_message_valid() {
    assert!(is_valid_message("Hello"));
    assert!(is_valid_message("a"));
    assert!(is_valid_message("This is a test message"));
  }

  #[test]
  fn test_is_valid_message_empty() {
    assert!(!is_valid_message(""));
  }
}
