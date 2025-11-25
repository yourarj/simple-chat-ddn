use anyhow::{Result, anyhow};
use chat_core::protocol::{ChatMessage, LENGTH_PREFIX, decode_message, encode_message};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tracing::{error, info};

/// Async CLI chat client with non-blocking I/O
pub struct ChatClient {
  username: String,
  reader: BufReader<OwnedReadHalf>,
  writer: OwnedWriteHalf,
}

impl ChatClient {
  /// Create new chat client and connect to server
  pub async fn new(host: &str, port: u16, username: &str) -> Result<Self> {
    let (reader, writer) = TcpStream::connect(format!("{}:{}", host, port))
      .await?
      .into_split();
    info!("Connected to server at {}:{}", host, port);

    let mut client = Self {
      reader: BufReader::new(reader),
      writer,
      username: username.to_string(),
    };

    // Send join message
    client.join().await?;

    Ok(client)
  }

  /// Main client event loop
  pub async fn run(&mut self) -> Result<()> {
    let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut buffer = vec![0u8; 4096];
    let mut stdin_buffer = String::new();

    println!("Welcome to the chat! Commands: 'send <message>', 'leave'");

    loop {
      select! {
          // Read from server
          result = self.reader.read(&mut buffer) => {
              match result {
                  Ok(0) => {
                      println!("Server disconnected");
                      break;
                  }
                  Ok(n) => {
                      if let Err(e) = self.handle_server_message(&buffer[..n]).await {
                          error!("Error handling server message: {}", e);
                      }
                  }
                  Err(e) => {
                      error!("Error reading from server: {}", e);
                      break;
                  }
              }
          }

          // Read from stdin
          result = stdin.read_line(&mut stdin_buffer) => {
              match result {
                  Ok(0) => {
                      println!("Stdin closed");
                      break;
                  }
                  Ok(_) => {
                      let input = stdin_buffer.trim();
                      if let Err(e) = self.handle_user_input(input).await {
                          error!("Error handling user input: {}", e);
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

    self.leave().await?;
    Ok(())
  }

  /// Handle incoming message from server
  async fn handle_server_message(&self, data: &[u8]) -> Result<()> {
    if data.len() < LENGTH_PREFIX {
      return Err(anyhow!("Message too short"));
    }

    let length = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < LENGTH_PREFIX + length {
      return Err(anyhow!("Incomplete message"));
    }

    let message = decode_message(&data[LENGTH_PREFIX..LENGTH_PREFIX + length])?;

    match message {
      ChatMessage::Message { username, content } => {
        println!("{}: {}", username, content);
      }
      ChatMessage::Error { reason } => {
        println!("Error: {}", reason);
      }
      ChatMessage::Success { message } => {
        println!("Success: {}", message);
      }
      _ => {
        println!("Unknown message type received");
      }
    }

    Ok(())
  }

  /// Handle user input from CLI
  async fn handle_user_input(&mut self, input: &str) -> Result<()> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();

    match parts[0] {
      "send" if parts.len() > 1 => {
        let message = ChatMessage::Message {
          username: self.username.clone(),
          content: parts[1].to_string(),
        };
        self.send_message(message).await?;
      }
      "leave" => {
        println!("Leaving chat...");
        let message = ChatMessage::Leave {
          username: self.username.clone(),
        };
        self.send_message(message).await?;
        return Err(anyhow!("Client requested leave")); // Break loop
      }
      _ => {
        println!("Unknown command. Use 'send <message>' or 'leave'");
      }
    }

    Ok(())
  }

  /// Send join message to server
  async fn join(&mut self) -> Result<()> {
    let message = ChatMessage::Join {
      username: self.username.clone(),
    };
    let frame = encode_message(&message)?;

    self.writer.write_all(&frame).await?;

    Ok(())
  }

  /// Send leave message to server
  async fn leave(&mut self) -> Result<()> {
    let message = ChatMessage::Leave {
      username: self.username.clone(),
    };
    let frame = encode_message(&message)?;

    self.writer.write_all(&frame).await?;

    Ok(())
  }

  /// Send generic message to server
  async fn send_message(&mut self, message: ChatMessage) -> Result<()> {
    let frame = encode_message(&message)?;
    self.writer.write_all(&frame).await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  //   use super::*;

  #[test]
  fn test_input_parsing() {
    // let client = ChatClient {
    //   stream: unimplemented!(),
    //   username: "test".to_string(),
    // };
  }
}
