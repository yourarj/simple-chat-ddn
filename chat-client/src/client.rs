use anyhow::{Result, anyhow};
use chat_core::protocol::{
  ClientMessage, LENGTH_PREFIX, ServerMessage, decode_message, encode_message,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::signal;
use tracing::{error, info, warn};

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
    let mut shutdown_signal = Box::pin(signal::ctrl_c());
    let mut should_shutdown = false;

    println!("Welcome to the chat! Commands: 'send <message>', 'leave'");
    info!("Client event loop started for user: {}", self.username);

    loop {
      select! {
          // Handle shutdown signal
          _ = shutdown_signal.as_mut() => {
              info!("Received shutdown signal (Ctrl+C)");
              should_shutdown = true;
              break;
          }

          // Read from server
          result = self.reader.read(&mut buffer) => {
              if should_shutdown {
                  break;
              }
              match result {
                  Ok(0) => {
                      info!("Server disconnected unexpectedly");
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

    // Graceful shutdown
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
      // Non-graceful exit (server disconnect, stdin close, etc.)
      warn!(
        "Client exiting without graceful shutdown for user: {}",
        self.username
      );
    }

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

    // Decode as ServerMessage
    let message = decode_message(&data[LENGTH_PREFIX..LENGTH_PREFIX + length])
      .map_err(|e| anyhow!("Failed to decode server message: {}", e))?;

    match message {
      ServerMessage::Message { username, content } => {
        println!("🗨️: {}: {}", username, content);
      }
      ServerMessage::Error { reason } => {
        println!("❌: {}", reason);
      }
      ServerMessage::Success { message } => {
        println!("❕: {}", message);
      }
      ServerMessage::UserJoined { username } => {
        println!("⚠️: `{}` joined the chat", username);
      }
      ServerMessage::UserLeft { username } => {
        println!("⚠️: `{}` left the chat", username);
      }
    }
    Ok(())
  }

  /// Handle user input from CLI
  async fn handle_user_input(&mut self, input: &str) -> Result<()> {
    let parts: Vec<&str> = input.splitn(2, ' ').collect();

    match parts[0] {
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
    let message = ClientMessage::Join {
      username: self.username.clone(),
    };
    self.send_client_message(message).await?;
    Ok(())
  }

  /// Send client message to server
  async fn send_client_message(&mut self, message: ClientMessage) -> Result<()> {
    let frame = encode_message(&message)?;
    self.writer.write_all(&frame).await?;
    Ok(())
  }

  /// Graceful shutdown with proper error handling and cleanup
  async fn graceful_leave(&mut self) -> Result<()> {
    info!("Sending leave message for user: {}", self.username);
    let message = ClientMessage::Leave {
      username: self.username.clone(),
    };
    self.send_client_message(message).await?;

    // Flush to ensure it's sent
    self.writer.flush().await?;

    info!(
      "Leave message sent successfully for user: {}",
      self.username
    );

    // Gracefully shutdown the writer
    match self.writer.shutdown().await {
      Ok(()) => info!("Writer shutdown successful for user: {}", self.username),
      Err(e) => warn!("Writer shutdown failed for user: {}: {}", self.username, e),
    }

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
