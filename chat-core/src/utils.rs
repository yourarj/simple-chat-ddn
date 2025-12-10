// chat-core/src/utils.rs

use crate::error::{ApplicationError, Result};

pub const MIN_USERNAME_LENGTH: usize = 3; // Should be at least 3
pub const MAX_USERNAME_LENGTH: usize = 30;

pub fn validate_username(username: &str) -> Result<()> {
  let trimmed = username.trim();

  if trimmed.is_empty() {
    return Err(ApplicationError::invalid_username(
      username.to_string(),
      "Username cannot be empty".to_string(),
    ));
  }

  if trimmed.len() < MIN_USERNAME_LENGTH {
    return Err(ApplicationError::invalid_username(
      username.to_string(),
      format!(
        "Username must be at least {} characters long",
        MIN_USERNAME_LENGTH
      ),
    ));
  }

  if trimmed.len() > MAX_USERNAME_LENGTH {
    return Err(ApplicationError::invalid_username(
      username.to_string(),
      format!(
        "Username must be at most {} characters long",
        MAX_USERNAME_LENGTH
      ),
    ));
  }

  // Only allow alphanumeric and underscore
  if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '_') {
    return Err(ApplicationError::invalid_username(
      username.to_string(),
      "Username can only contain letters, numbers, and underscores".to_string(),
    ));
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_valid_username() {
    assert!(validate_username("alice").is_ok());
    assert!(validate_username("bob123").is_ok());
    assert!(validate_username("user_name").is_ok());
  }

  #[test]
  fn test_invalid_username_too_short() {
    assert!(validate_username("ab").is_err());
    assert!(validate_username("a").is_err());
  }

  #[test]
  fn test_invalid_username_too_long() {
    let long_name = "a".repeat(31);
    assert!(validate_username(&long_name).is_err());
  }

  #[test]
  fn test_invalid_username_special_chars() {
    assert!(validate_username("user@name").is_err());
    assert!(validate_username("user name").is_err());
    assert!(validate_username("user-name").is_err());
  }

  #[test]
  fn test_invalid_username_empty() {
    assert!(validate_username("").is_err());
    assert!(validate_username("   ").is_err());
  }
}
