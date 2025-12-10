use chat_core::{
  error::{ApplicationError, Result},
  message_cache::MessageCache,
  protocol::{ClientMessage, ServerMessage, SharedServerMessage, encode_message},
  transport_layer::read_message_from_stream,
  utils,
};
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tracing::{debug, info, warn};

use crate::broadcast_pool::BroadcastPool;

const MAX_JOIN_ATTEMPTS: usize = 3;

pub async fn handle_client(
  stream: TcpStream,
  broadcast_pool: BroadcastPool,
  cache: MessageCache,
) -> Result<()> {
  let peer_addr = stream.peer_addr().ok();
  debug!("Handling client from: {:?}", peer_addr);

  let (mut reader, mut writer) = stream.into_split();
  let mut buffer = Vec::with_capacity(4096);
  let mut join_attempts = 0;

  // Join phase
  let username = loop {
    if join_attempts >= MAX_JOIN_ATTEMPTS {
      let error_msg =
        ServerMessage::error("Too many failed join attempts. Connection closed.".to_string());
      let frame = encode_message(&error_msg)?;
      writer.write_all(&frame).await?;
      writer.shutdown().await?;
      return Err(ApplicationError::config_error(
        "Max join attempts exceeded".to_string(),
      ));
    }

    join_attempts += 1;

    match read_message_from_stream(&mut reader, &mut buffer).await {
      Ok(ClientMessage::Join { username: un }) => match utils::validate_username(&un) {
        Ok(()) => {
          if broadcast_pool.has(&un) {
            let error_msg = ServerMessage::user_name_already_taken(un.clone());
            let frame = encode_message(&error_msg)?;
            writer.write_all(&frame).await?;
            continue;
          }
          break un;
        }
        Err(e) => {
          let error_msg = ServerMessage::error(format!("Invalid username: {}", e));
          let frame = encode_message(&error_msg)?;
          writer.write_all(&frame).await?;
          continue;
        }
      },
      Ok(_) => {
        let error_msg =
          ServerMessage::error("You must join the chat first. Send a Join message.".to_string());
        let frame = encode_message(&error_msg)?;
        writer.write_all(&frame).await?;
      }
      Err(ApplicationError::ClientReadStreamClosed) => {
        debug!("Client disconnected before joining");
        return Ok(());
      }
      Err(e) => {
        warn!("Error reading join message: {}", e);
        let error_msg = ServerMessage::error(format!("Protocol error: {}", e));
        let frame = encode_message(&error_msg)?;
        writer.write_all(&frame).await?;
      }
    }
  };

  info!("User '{}' authenticated", username);

  // Create dispatcher with per-user counter
  broadcast_pool.create_dispatcher(username.clone(), writer, cache);

  // Get user's counter for creating messages
  let counter = broadcast_pool
    .get_counter(&username)
    .ok_or_else(|| ApplicationError::config_error("Counter not found".to_string()))?;

  // Broadcast join notification (uses user's counter)
  let join_notification =
    SharedServerMessage::new_with_counter(ServerMessage::user_joined(username.clone()), &counter);
  broadcast_pool.broadcast_message(join_notification).await?;

  // Message handling loop
  loop {
    match read_message_from_stream(&mut reader, &mut buffer).await {
      Ok(ClientMessage::Join { .. }) => {
        debug!("User '{}' sent duplicate join message", username);
      }
      Ok(ClientMessage::Leave {
        username: leave_user,
      }) => {
        if leave_user != username {
          debug!("User '{}' tried to leave as '{}'", username, leave_user);
          continue;
        }

        info!("User '{}' requested to leave", username);

        // Use user's counter for leave message
        let leave_msg = SharedServerMessage::new_with_counter(
          ServerMessage::user_left(username.clone()),
          &counter,
        );
        broadcast_pool.broadcast_message(leave_msg).await?;
        broadcast_pool.destroy_dispatcher(&username);
        break;
      }
      Ok(ClientMessage::Message {
        username: msg_user,
        content,
      }) => {
        if msg_user != username {
          debug!(
            "User '{}' tried to send message as '{}'",
            username, msg_user
          );
          continue;
        }

        if content.trim().is_empty() {
          debug!("User '{}' sent empty message", username);
          continue;
        }

        debug!("Message from '{}': {}", username, content);

        // Use user's counter for chat message
        let msg = SharedServerMessage::new_with_counter(
          ServerMessage::message(username.clone(), content),
          &counter,
        );
        broadcast_pool.broadcast_message(msg).await?;
      }
      Err(ApplicationError::ClientReadStreamClosed) => {
        info!("Client '{}' disconnected", username);
        break;
      }
      Err(e) => {
        warn!("Error reading message from '{}': {}", username, e);
        break;
      }
    }
  }

  // Cleanup
  broadcast_pool.destroy_dispatcher(&username);
  debug!("Client handler terminated for '{}'", username);
  Ok(())
}
