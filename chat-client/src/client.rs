use chat_core::{
  error::{ApplicationError, Result},
  protocol::{
    ClientMessage, LENGTH_PREFIX, MAX_MESSAGE_SIZE, ServerMessage, decode_message, encode_message,
  },
};
use std::io::{self, Write};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter},
  net::TcpStream,
  sync::mpsc,
};
use tracing::{debug, error, info, warn};

pub async fn run_client(host: &str, port: u16, username: &str) -> Result<()> {
  // Connect to server
  let addr = format!("{}:{}", host, port);
  let stream = TcpStream::connect(&addr).await.map_err(|e| {
    error!("Failed to connect to {}: {}", addr, e);
    ApplicationError::Io(e)
  })?;

  info!("Connected to server at {}", addr);

  let (reader, writer) = stream.into_split();
  let mut reader = BufReader::new(reader);
  let mut writer = BufWriter::new(writer);

  // Send join message
  let join_msg = ClientMessage::Join {
    username: username.to_string(),
  };
  let frame = encode_message(&join_msg)?;
  writer.write_all(&frame).await?;
  writer.flush().await?;

  info!("Join request sent for username: {}", username);

  // Create channel for user input
  let (input_tx, mut input_rx) = mpsc::unbounded_channel::<String>();

  // Spawn input reader task
  let input_task = tokio::task::spawn_blocking(move || read_user_input(input_tx));

  // Main event loop
  let result = loop {
    tokio::select! {
        // Handle server messages
        read_result = read_server_message(&mut reader) => {
            match read_result {
                Ok(msg) => {
                    display_server_message(&msg);

                    // Check for error messages that should terminate
                    if matches!(msg, ServerMessage::Error { .. }) {
                        warn!("Received error from server, consider reconnecting");
                    }
                }
                Err(ApplicationError::ClientReadStreamClosed) => {
                    info!("Server closed connection");
                    break Ok(());
                }
                Err(e) => {
                    error!("Error reading from server: {}", e);
                    break Err(e);
                }
            }
        }

        // Handle user input
        input = input_rx.recv() => {
            match input {
                Some(line) => {
                    match handle_user_input(&line, username, &mut writer).await {
                        Ok(should_exit) => {
                            if should_exit {
                                info!("Exiting client...");
                                break Ok(());
                            }
                        }
                        Err(e) => {
                            error!("Error sending message: {}", e);
                            break Err(e);
                        }
                    }
                }
                None => {
                    debug!("Input channel closed");
                    break Ok(());
                }
            }
        }
    }
  };

  // Cleanup
  input_task.abort();

  result
}

/// Read user input from stdin in a blocking task
fn read_user_input(tx: mpsc::UnboundedSender<String>) -> Result<()> {
  let stdin = io::stdin();
  let mut input = String::new();

  print_prompt();

  loop {
    input.clear();

    match stdin.read_line(&mut input) {
      Ok(0) => {
        // EOF reached
        debug!("EOF on stdin");
        break;
      }
      Ok(_) => {
        let line = input.trim().to_string();
        if !line.is_empty() && tx.send(line).is_err() {
          debug!("Failed to send input, receiver dropped");
          break;
        }
        print_prompt();
      }
      Err(e) => {
        error!("Error reading from stdin: {}", e);
        break;
      }
    }
  }

  Ok(())
}

/// Handle user input command
async fn handle_user_input(
  input: &str,
  username: &str,
  writer: &mut BufWriter<tokio::net::tcp::OwnedWriteHalf>,
) -> Result<bool> {
  let parts: Vec<&str> = input.splitn(2, ' ').collect();

  match parts.as_slice() {
    ["send", message] if !message.trim().is_empty() => {
      let msg = ClientMessage::Message {
        username: username.to_string(),
        content: message.trim().to_string(),
      };
      let frame = encode_message(&msg)?;
      writer.write_all(&frame).await?;
      writer.flush().await?;
      Ok(false)
    }
    ["send"] | ["send", _] => {
      println!("Usage: send <message>");
      Ok(false)
    }
    ["leave"] => {
      let msg = ClientMessage::Leave {
        username: username.to_string(),
      };
      let frame = encode_message(&msg)?;
      writer.write_all(&frame).await?;
      writer.flush().await?;
      Ok(true)
    }
    ["help"] => {
      print_help();
      Ok(false)
    }
    _ => {
      println!(
        "Unknown command: '{}'. Type 'help' for available commands.",
        input
      );
      Ok(false)
    }
  }
}

/// Read a server message from the stream
async fn read_server_message(
  reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>,
) -> Result<ServerMessage> {
  // Read length prefix
  let mut length_bytes = [0u8; LENGTH_PREFIX];
  let n = reader.read_exact(&mut length_bytes).await?;

  if n == 0 {
    return Err(ApplicationError::ClientReadStreamClosed);
  }

  let payload_len = u32::from_be_bytes(length_bytes) as usize;

  if payload_len > MAX_MESSAGE_SIZE {
    return Err(ApplicationError::message_too_large(
      payload_len,
      MAX_MESSAGE_SIZE,
    ));
  }

  if payload_len == 0 {
    return Err(ApplicationError::invalid_frame(
      "Zero-length payload".to_string(),
    ));
  }

  // Read payload
  let mut payload = vec![0u8; payload_len];
  reader.read_exact(&mut payload).await?;

  decode_message(&payload)
}

/// Display server message to user
fn display_server_message(msg: &ServerMessage) {
  match msg {
    ServerMessage::Success { message } => {
      println!("\n✓ {}", message);
    }
    ServerMessage::Error { reason } => {
      println!("\n✗ Error: {}", reason);
    }
    ServerMessage::Message { username, content } => {
      println!("\n[{}]: {}", username, content);
    }
    ServerMessage::UserJoined { username } => {
      println!("\n→ {} joined the chat", username);
    }
    ServerMessage::UserLeft { username } => {
      println!("\n← {} left the chat", username);
    }
  }
  print_prompt();
}

fn print_prompt() {
  print!("> ");
  let _ = io::stdout().flush();
}

fn print_help() {
  println!("\nAvailable commands:");
  println!("  send <message>  - Send a message to the chat");
  println!("  leave           - Leave the chat and disconnect");
  println!("  help            - Show this help message");
}
