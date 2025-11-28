use anyhow::{Result, anyhow};
use chat_core::protocol::{ClientMessage, ServerMessage, encode_message};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::signal;
use tracing::{error, info, warn};

pub struct ChatClient {
  username: String,
  original_username: String,
  reader: BufReader<OwnedReadHalf>,
  writer: OwnedWriteHalf,
  join_attempts: u8,
}

impl ChatClient {
  pub async fn new(host: &str, port: u16, username: &str) -> Result<Self> {
    let (reader, writer) = TcpStream::connect(format!("{}:{}", host, port))
      .await?
      .into_split();
    info!("Connected to server at {}:{}", host, port);

    let mut client = Self {
      reader: BufReader::new(reader),
      writer,
      username: username.to_string(),
      original_username: username.to_string(),
      join_attempts: 0,
    };

    client.join_with_retry().await?;

    Ok(client)
  }

  pub async fn run(&mut self) -> Result<()> {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut buffer = vec![0u8; 4096];
    let mut stdin_buffer = String::new();
    let mut shutdown_signal = Box::pin(signal::ctrl_c());
    let mut should_shutdown = false;

    println!();
    println!("Welcome to the chat! Commands: 'send <message>', 'leave'");
    info!("Client event loop started for user: {}", self.username);

    loop {
      select! {

          _ = shutdown_signal.as_mut() => {
              info!("Received shutdown signal (Ctrl+C)");
              should_shutdown = true;
              break;
          }


          result = self.reader.read(&mut buffer) => {
              if should_shutdown {
                  break;
              }
              match result {
                  Ok(0) => {
                      info!("Server disconnected unexpectedly");
                      println!();
                      println!("❌❌: Server disconnected!❌❌ press ENTER ↩️ to exit!");
                      println!();
                      break;
                  }
                  Ok(n) => {
                      if let Err(e) = crate::message_handler::handle_server_message(&buffer[..n]).await {
                          error!("Error handling server message: {}", e);
                      }
                  }
                  Err(e) => {
                      error!("Error reading from server: {}", e);
                      break;
                  }
              }
          }


          result = stdin.read_line(&mut stdin_buffer) => {
              if should_shutdown {
                  break;
              }
              match result {
                  Ok(0) => {
                      info!("Stdin closed unexpectedly");
                      println!("Stdin closed");
                      break;
                  }
                  Ok(_) => {
                      let input = stdin_buffer.trim();
                      if let Err(e) = self.handle_user_input(input).await {
                          if e.to_string().contains("Client requested leave") {
                              info!("User requested graceful shutdown via 'leave' command");
                              should_shutdown = true;
                              break;
                          } else {
                              error!("Error handling user input: {}", e);
                          }
                      }
                      stdin_buffer.clear();
                  }
                  Err(e) => {
                      error!("Error reading from stdin: {}", e);
                      break;
                  }
              }
          }
      }
    }

    if should_shutdown {
      info!("Initiating graceful shutdown for user: {}", self.username);
      if let Err(e) = self.graceful_leave().await {
        error!("Failed to send leave message during shutdown: {}", e);
      } else {
        info!(
          "Successfully sent leave message for user: {}",
          self.username
        );
      }
    } else {
      warn!(
        "Client exiting without graceful shutdown for user: {}",
        self.username
      );
    }

    Ok(())
  }

  async fn handle_user_input(&mut self, input: &str) -> Result<()> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();

    match parts[0] {
      "send" if parts.len() == 1 => {
        println!("No message provided. Use 'send <message>'");
      }
      "send" if parts.len() > 1 => {
        info!("User {} sending message: {}", self.username, parts[1]);
        let message = ClientMessage::Message {
          username: self.username.clone(),
          content: parts[1].to_string(),
        };
        self.send_client_message(message).await?;
      }
      "leave" => {
        info!("User {} requested leave via command", self.username);
        println!("Leaving chat...");
        let message = ClientMessage::Leave {
          username: self.username.clone(),
        };
        self.send_client_message(message).await?;
        return Err(anyhow!("Client requested leave"));
      }
      _ => {
        println!("Unknown command. Use 'send <message>' or 'leave'");
      }
    }

    Ok(())
  }

  pub async fn send_client_message(&mut self, message: ClientMessage) -> Result<()> {
    let frame = encode_message(&message)?;
    self.writer.write_all(&frame).await?;
    Ok(())
  }

  pub async fn graceful_leave(&mut self) -> Result<()> {
    info!("Sending leave message for user: {}", self.username);
    let message = ClientMessage::Leave {
      username: self.username.clone(),
    };
    self.send_client_message(message).await?;

    self.writer.flush().await?;

    info!(
      "Leave message sent successfully for user: {}",
      self.username
    );

    match self.writer.shutdown().await {
      Ok(()) => info!("Writer shutdown successful for user: {}", self.username),
      Err(e) => warn!("Writer shutdown failed for user: {}: {}", self.username, e),
    }

    Ok(())
  }

  /// Join the chat with retry
  async fn join_with_retry(&mut self) -> Result<()> {
    const MAX_JOIN_ATTEMPTS: u8 = 3;

    loop {
      self.join_attempts += 1;

      info!(
        "Attempting to join with username: {} (attempt {}/{})",
        self.username, self.join_attempts, MAX_JOIN_ATTEMPTS
      );

      let message = ClientMessage::Join {
        username: self.username.clone(),
      };
      self.send_client_message(message).await?;

      let mut buffer = vec![0u8; 4096];

      let reader = self.reader.get_mut();
      match chat_core::transport_layer::read_message_from_stream(reader, &mut buffer).await {
        Ok(ServerMessage::Success { message }) => {
          info!("Successfully joined chat: {}", message);
          println!("✅ {}", message);
          return Ok(());
        }
        Ok(ServerMessage::UserNameAlreadyTaken { username }) => {
          warn!("username '{}' is already taken", username);

          if self.join_attempts >= MAX_JOIN_ATTEMPTS {
            return Err(anyhow!(
              "Failed to join after {} attempts. username '{}' is taken and no alternatives were accepted.",
              MAX_JOIN_ATTEMPTS,
              username
            ));
          }

          let suggested_username = crate::username_handler::generate_alternative_username(
            &self.username,
            self.join_attempts,
          );
          match crate::username_handler::prompt_username_selection_loop(
            &self.original_username,
            &suggested_username,
          )
          .await?
          {
            Some(new_username) => {
              self.username = new_username;
            }
            None => {
              return Err(anyhow!("User cancelled join process"));
            }
          }
        }
        Ok(ServerMessage::Error { reason }) => {
          return Err(anyhow!("Server error during join: {}", reason));
        }
        Ok(_) => {
          return Err(anyhow!("Unexpected server response during join process"));
        }
        Err(e) => {
          return Err(anyhow!("Application error: {}", e));
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {

  use chat_core::protocol::{ClientMessage, ServerMessage, decode_message, encode_message};

  #[test]
  fn test_client_message_username_extraction() {
    let join_message = ClientMessage::Join {
      username: "alice".to_string(),
    };
    assert_eq!(join_message.username(), Some("alice"));

    let leave_message = ClientMessage::Leave {
      username: "bob".to_string(),
    };
    assert_eq!(leave_message.username(), Some("bob"));

    let message_message = ClientMessage::Message {
      username: "charlie".to_string(),
      content: "Hello".to_string(),
    };
    assert_eq!(message_message.username(), Some("charlie"));
  }

  #[tokio::test]
  async fn test_client_message_serialization() {
    let messages = vec![
      ClientMessage::Join {
        username: "alice".to_string(),
      },
      ClientMessage::Leave {
        username: "bob".to_string(),
      },
      ClientMessage::Message {
        username: "charlie".to_string(),
        content: "Hello, world!".to_string(),
      },
    ];

    for message in messages {
      let encoded = encode_message(&message).expect("Failed to encode client message");
      assert!(
        encoded.len() > 4,
        "Encoded message should have length prefix and payload"
      );

      let payload = &encoded[4..];

      let decoded: ClientMessage =
        decode_message(payload).expect("Failed to decode client message");

      match (&message, decoded) {
        (
          ClientMessage::Join {
            username: orig_user,
          },
          ClientMessage::Join { username: dec_user },
        ) => {
          assert_eq!(orig_user, &dec_user);
        }
        (
          ClientMessage::Leave {
            username: orig_user,
          },
          ClientMessage::Leave { username: dec_user },
        ) => {
          assert_eq!(orig_user, &dec_user);
        }
        (
          ClientMessage::Message {
            username: orig_user,
            content: orig_content,
          },
          ClientMessage::Message {
            username: dec_user,
            content: dec_content,
          },
        ) => {
          assert_eq!(orig_user, &dec_user);
          assert_eq!(orig_content, &dec_content);
        }
        _ => panic!("Message type mismatch after encoding/decoding"),
      }
    }
  }

  #[tokio::test]
  async fn test_server_message_handling() {
    let test_cases = vec![
      ServerMessage::Message {
        username: "alice".to_string(),
        content: "Hello everyone!".to_string(),
      },
      ServerMessage::Error {
        reason: "Username already taken".to_string(),
      },
      ServerMessage::Success {
        message: "Welcome to the chat!".to_string(),
      },
      ServerMessage::UserJoined {
        username: "newuser".to_string(),
      },
      ServerMessage::UserLeft {
        username: "leavinguser".to_string(),
      },
    ];

    for message in test_cases {
      let encoded = encode_message(&message).expect("Failed to encode server message");
      assert!(
        encoded.len() > 4,
        "Encoded message should have length prefix and payload"
      );

      let payload = &encoded[4..];

      let decoded: ServerMessage =
        decode_message(payload).expect("Failed to decode server message");

      match (&message, decoded) {
        (ServerMessage::Message { .. }, ServerMessage::Message { .. }) => {}
        (ServerMessage::Error { .. }, ServerMessage::Error { .. }) => {}
        (ServerMessage::Success { .. }, ServerMessage::Success { .. }) => {}
        (ServerMessage::UserJoined { .. }, ServerMessage::UserJoined { .. }) => {}
        (ServerMessage::UserLeft { .. }, ServerMessage::UserLeft { .. }) => {}
        _ => panic!("Server message type mismatch after encoding/decoding"),
      }
    }
  }

  #[test]
  fn test_message_length_prefix() {
    let test_message = ClientMessage::Message {
      username: "test".to_string(),
      content: "Hello".to_string(),
    };

    let encoded = encode_message(&test_message).expect("Failed to encode test message");

    assert!(
      encoded.len() > 4,
      "Message should have length prefix and payload"
    );

    let length_bytes = &encoded[0..4];
    let length = u32::from_be_bytes([
      length_bytes[0],
      length_bytes[1],
      length_bytes[2],
      length_bytes[3],
    ]) as usize;

    assert_eq!(
      length,
      encoded.len() - 4,
      "Length prefix should match payload size"
    );
  }

  #[test]
  fn test_large_message_serialization() {
    let large_content = "x".repeat(10000);
    let test_message = ClientMessage::Message {
      username: "largeuser".to_string(),
      content: large_content,
    };

    let encoded = encode_message(&test_message).expect("Failed to encode large message");
    assert!(
      encoded.len() > 10000,
      "Large message should result in substantial encoded size"
    );

    let payload = &encoded[4..];

    let decoded: ClientMessage = decode_message(payload).expect("Failed to decode large message");

    match decoded {
      ClientMessage::Message { username, content } => {
        assert_eq!(username, "largeuser");
        assert_eq!(content.len(), 10000);
      }
      _ => panic!("Expected large ClientMessage::Message"),
    }
  }

  #[test]
  fn test_empty_message_serialization() {
    let test_message = ClientMessage::Message {
      username: "".to_string(),
      content: "".to_string(),
    };

    let encoded = encode_message(&test_message).expect("Failed to encode empty message");
    assert!(
      encoded.len() >= 4,
      "Message should have at least length prefix"
    );

    let payload = &encoded[4..];

    let decoded: ClientMessage = decode_message(payload).expect("Failed to decode empty message");

    match decoded {
      ClientMessage::Message { username, content } => {
        assert_eq!(username, "");
        assert_eq!(content, "");
      }
      _ => panic!("Expected empty ClientMessage::Message"),
    }
  }

  #[test]
  fn test_message_equality() {
    let msg1 = ClientMessage::Message {
      username: "alice".to_string(),
      content: "Hello".to_string(),
    };

    let msg2 = ClientMessage::Message {
      username: "alice".to_string(),
      content: "Hello".to_string(),
    };

    let encoded1 = encode_message(&msg1).expect("Failed to encode first message");
    let encoded2 = encode_message(&msg2).expect("Failed to encode second message");

    assert_eq!(
      encoded1, encoded2,
      "Identical messages should produce identical encoded data"
    );
  }

  #[test]
  fn test_username_validation_edge_cases() {
    let valid_usernames: Vec<String> = vec![
      "alice".to_string(),
      "user123".to_string(),
      "test_user".to_string(),
      "a".repeat(50),
    ];

    let edge_case_usernames: Vec<String> = vec!["".to_string(), "a".repeat(1000)];

    for username in valid_usernames {
      let message = ClientMessage::Join { username };
      let encoded = encode_message(&message).expect("Should encode valid username");
      assert!(encoded.len() > 4);
    }

    for username in edge_case_usernames {
      let message = ClientMessage::Join { username };
      let encoded = encode_message(&message).expect("Should encode edge case username");
      assert!(encoded.len() > 4);
    }
  }

  #[test]
  fn test_content_validation_edge_cases() {
    let valid_contents: Vec<String> = vec![
      "Hello world".to_string(),
      "".to_string(),
      "a".repeat(1000),
      "Special chars: !@#$%^&*()".to_string(),
      "Unicode: 🚀✨🎉".to_string(),
    ];

    for content in valid_contents {
      let message = ClientMessage::Message {
        username: "testuser".to_string(),
        content: content.clone(),
      };
      let encoded = encode_message(&message).expect("Should encode valid content");
      assert!(encoded.len() > 4);

      let payload = &encoded[4..];
      let decoded: ClientMessage = decode_message(payload).expect("Should decode message");

      match decoded {
        ClientMessage::Message {
          content: decoded_content,
          ..
        } => {
          assert_eq!(content, decoded_content);
        }
        _ => panic!("Expected Message variant"),
      }
    }
  }

  #[test]
  fn test_command_parsing_logic() {
    let test_cases = vec![
      ("send Hello world", Some(("send", "Hello world"))),
      ("send ", Some(("send", ""))),
      ("leave", None),
      ("unknown", None),
      ("", None),
      ("send", None),
    ];

    for (input, expected) in test_cases {
      let parts: Vec<&str> = input.splitn(2, ' ').collect();

      match expected {
        Some((command, content)) => {
          assert_eq!(
            parts.len(),
            2,
            "Input '{}' should split into 2 parts",
            input
          );
          assert_eq!(
            parts[0], command,
            "First part should be command for input '{}'",
            input
          );
          assert_eq!(
            parts[1], content,
            "Second part should be content for input '{}'",
            input
          );
        }
        None => {
          assert!(
            !parts.is_empty(),
            "Input '{}' should have at least 1 part",
            input
          );
        }
      }
    }
  }
}
