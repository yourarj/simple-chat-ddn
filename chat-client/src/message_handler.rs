//! Client-side message handling and display
//!
//! This module handles receiving messages from server, sending messages,
//! and formatting display output for the user.

use chat_core::{
  error::{ApplicationError, Result},
  protocol::{ClientMessage, ServerMessage, encode_message},
};
use std::io::{self, Write};
use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tracing::{debug, error, info};

use crate::username_handler::read_server_message;

/// Message display styles
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayStyle {
  /// Success message (green)
  Success,
  /// Error message (red)
  Error,
  /// Chat message from user
  ChatMessage,
  /// System notification (user join/leave)
  SystemNotification,
}

/// Send a chat message to the server
///
/// # Arguments
/// * `writer` - TCP writer to send to
/// * `username` - Sender's username
/// * `content` - Message content
pub async fn send_chat_message(
  writer: &mut BufWriter<OwnedWriteHalf>,
  username: &str,
  content: &str,
) -> Result<()> {
  if content.trim().is_empty() {
    return Err(ApplicationError::invalid_frame(
      "Message content cannot be empty".to_string(),
    ));
  }

  debug!("Sending message: {}", content);

  let msg = ClientMessage::Message {
    username: username.to_string(),
    content: content.trim().to_string(),
  };

  let frame = encode_message(&msg)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;

  Ok(())
}

/// Display a server message to the user with appropriate formatting
///
/// # Arguments
/// * `msg` - The server message to display
pub fn display_server_message(msg: &ServerMessage) {
  match msg {
    ServerMessage::Success { message } => {
      display_formatted_message(message, DisplayStyle::Success);
    }
    ServerMessage::Error { reason } => {
      display_formatted_message(&format!("Error: {}", reason), DisplayStyle::Error);
    }
    ServerMessage::Message { username, content } => {
      display_chat_message(username, content);
    }
    ServerMessage::UserJoined { username } => {
      display_formatted_message(
        &format!("{} joined the chat", username),
        DisplayStyle::SystemNotification,
      );
    }
    ServerMessage::UserLeft { username } => {
      display_formatted_message(
        &format!("{} left the chat", username),
        DisplayStyle::SystemNotification,
      );
    }
  }
  print_prompt();
}

/// Display a formatted message with style
fn display_formatted_message(message: &str, style: DisplayStyle) {
  match style {
    DisplayStyle::Success => {
      println!("\n✓ {}", message);
    }
    DisplayStyle::Error => {
      println!("\n✗ {}", message);
    }
    DisplayStyle::SystemNotification => {
      println!("\n→ {}", message);
    }
    DisplayStyle::ChatMessage => {
      println!("\n{}", message);
    }
  }
}

/// Display a chat message with username
fn display_chat_message(username: &str, content: &str) {
  println!("\n[{}]: {}", username, content);
}

/// Print the input prompt
pub fn print_prompt() {
  print!("> ");
  let _ = io::stdout().flush();
}

/// Print help message showing available commands
pub fn print_help() {
  println!("\n╔════════════════════════════════════════╗");
  println!("║          Available Commands            ║");
  println!("╠════════════════════════════════════════╣");
  println!("║ send <message> - Send a chat message   ║");
  println!("║ leave          - Leave the chat        ║");
  println!("║ help           - Show this help        ║");
  println!("║ clear          - Clear the screen      ║");
  println!("╚════════════════════════════════════════╝");
}

/// Print welcome banner
pub fn print_welcome_banner(username: &str) {
  println!("\n╔════════════════════════════════════════╗");
  println!("║        Welcome to Rust Chat!           ║");
  println!("╠════════════════════════════════════════╣");
  println!("║ Username: {:28} ║", username);
  println!("║ Type 'help' for available commands    ║");
  println!("╚════════════════════════════════════════╝\n");
}

/// Clear the terminal screen
pub fn clear_screen() {
  print!("\x1B[2J\x1B[1;1H");
  let _ = io::stdout().flush();
}

/// Process incoming server messages continuously
///
/// # Arguments
/// * `reader` - TCP reader to receive messages from
///
/// # Returns
/// Ok(()) when connection closes normally, Err on error
pub async fn process_server_messages(reader: &mut BufReader<OwnedReadHalf>) -> Result<()> {
  loop {
    match read_server_message(reader).await {
      Ok(msg) => {
        display_server_message(&msg);

        // Check for critical errors that should terminate
        if let ServerMessage::Error { reason } = &msg
          && (reason.contains("Connection closed")
            || reason.contains("Too many")
            || reason.contains("banned"))
        {
          error!("Terminal error from server: {}", reason);
          return Err(ApplicationError::config_error(reason.clone()));
        }
      }
      Err(ApplicationError::ClientReadStreamClosed) => {
        info!("Server closed connection");
        println!("\n✗ Disconnected from server");
        return Ok(());
      }
      Err(e) => {
        error!("Error reading from server: {}", e);
        return Err(e);
      }
    }
  }
}

/// Parse and validate user input command
///
/// # Arguments
/// * `input` - Raw user input string
///
/// # Returns
/// Parsed command and arguments
pub fn parse_user_command(input: &str) -> Result<UserCommand> {
  let input = input.trim();

  if input.is_empty() {
    return Err(ApplicationError::invalid_frame("Empty command".to_string()));
  }

  let parts: Vec<&str> = input.splitn(2, ' ').collect();

  match parts.as_slice() {
    ["send", message] if !message.trim().is_empty() => {
      Ok(UserCommand::SendMessage(message.trim().to_string()))
    }
    ["send"] | ["send", _] => Err(ApplicationError::invalid_frame(
      "Usage: send <message>".to_string(),
    )),
    ["leave"] => Ok(UserCommand::Leave),
    ["help"] => Ok(UserCommand::Help),
    ["clear"] => Ok(UserCommand::Clear),
    _ => Err(ApplicationError::invalid_frame(format!(
      "Unknown command: '{}'. Type 'help' for available commands.",
      parts[0]
    ))),
  }
}

/// User command types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserCommand {
  SendMessage(String),
  Leave,
  Help,
  Clear,
}

/// Handle a user command
///
/// # Arguments
/// * `command` - The parsed user command
/// * `username` - Current user's username
/// * `writer` - TCP writer to send messages to
///
/// # Returns
/// Ok(true) if should exit, Ok(false) to continue, Err on error
pub async fn handle_user_command(
  command: UserCommand,
  username: &str,
  writer: &mut BufWriter<OwnedWriteHalf>,
) -> Result<bool> {
  match command {
    UserCommand::SendMessage(content) => {
      send_chat_message(writer, username, &content).await?;
      Ok(false)
    }
    UserCommand::Leave => {
      info!("User requested to leave");
      let leave_msg = ClientMessage::Leave {
        username: username.to_string(),
      };
      let frame = encode_message(&leave_msg)?;
      writer.write_all(&frame).await?;
      writer.flush().await?;
      Ok(true) // Signal exit
    }
    UserCommand::Help => {
      print_help();
      Ok(false)
    }
    UserCommand::Clear => {
      clear_screen();
      Ok(false)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_parse_send_command() {
    let cmd = parse_user_command("send Hello world").expect("Should parse");
    assert_eq!(cmd, UserCommand::SendMessage("Hello world".to_string()));
  }

  #[test]
  fn test_parse_leave_command() {
    let cmd = parse_user_command("leave").expect("Should parse");
    assert_eq!(cmd, UserCommand::Leave);
  }

  #[test]
  fn test_parse_help_command() {
    let cmd = parse_user_command("help").expect("Should parse");
    assert_eq!(cmd, UserCommand::Help);
  }

  #[test]
  fn test_parse_invalid_command() {
    let result = parse_user_command("invalid");
    assert!(result.is_err());
  }

  #[test]
  fn test_parse_empty_send() {
    let result = parse_user_command("send   ");
    assert!(result.is_err());
  }

  #[test]
  fn test_display_style_equality() {
    assert_eq!(DisplayStyle::Success, DisplayStyle::Success);
    assert_ne!(DisplayStyle::Success, DisplayStyle::Error);
  }
}
