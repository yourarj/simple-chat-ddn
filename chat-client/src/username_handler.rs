use anyhow::anyhow;

/// Generate alternative usernames by appending numbers
pub fn generate_alternative_username(username: &str, attempts: u8) -> String {
  format!("{}_{:02}", username, attempts + 1)
}

/// Read user input with a prompt
async fn read_user_input(prompt: &str) -> anyhow::Result<String> {
  print!("{}", prompt);
  std::io::Write::flush(&mut std::io::stdout())?;

  let mut input = String::new();
  std::io::stdin()
    .read_line(&mut input)
    .map_err(|e| anyhow!("Failed to read input: {}", e))?;

  Ok(input.trim().to_string())
}

pub async fn prompt_username_selection_loop(
  taken: &str,
  suggested: &str,
) -> anyhow::Result<Option<String>> {
  loop {
    println!();
    println!("Username '{}' is already taken.", taken);
    println!("Suggested alternative: '{}'", suggested);
    println!();
    println!("Options:");
    println!("  1. Use suggested username: '{}'", suggested);
    println!("  2. Enter a custom username");
    println!("  3. Cancel and exit");
    print!("Please choose (1-3) or enter custom username directly: ");
    std::io::Write::flush(&mut std::io::stdout())?;

    let choice = read_user_input("").await?;

    match choice.as_str() {
      "1" => {
        println!("Using suggested username: '{}'", suggested);
        return Ok(Some(suggested.to_string()));
      }
      "2" => {
        let custom = read_user_input("Enter your desired username: ").await?;
        if !custom.is_empty() {
          println!("Using custom username: '{}'", custom);
          return Ok(Some(custom));
        }
        println!("Username cannot be empty. Please try again.");
      }
      "3" => {
        println!("Join cancelled by user.");
        return Ok(None);
      }
      _ if !choice.is_empty() => {
        println!("Using custom username: '{}'", choice);
        return Ok(Some(choice));
      }
      _ => {
        println!("Invalid choice. Please try again.");
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_alternative_username() {
    let username = "testuser";
    let alternative = generate_alternative_username(username, 0);
    assert_eq!(alternative, "testuser_01");

    let alternative = generate_alternative_username(username, 1);
    assert_eq!(alternative, "testuser_02");

    let alternative = generate_alternative_username(username, 9);
    assert_eq!(alternative, "testuser_10");
  }

  #[test]
  fn test_generate_alternative_username_with_special_chars() {
    let username = "user@domain";
    let alternative = generate_alternative_username(username, 0);
    assert_eq!(alternative, "user@domain_01");
  }

  #[test]
  fn test_generate_alternative_username_empty() {
    let username = "";
    let alternative = generate_alternative_username(username, 0);
    assert_eq!(alternative, "_01");
  }
}
