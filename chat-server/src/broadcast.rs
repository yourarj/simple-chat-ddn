use chat_core::{
  error::ApplicationError,
  protocol::{ServerMessage, encode_message},
};
use tokio::{
  io::AsyncWriteExt,
  net::tcp::OwnedWriteHalf,
  sync::broadcast::{Receiver, error::RecvError},
  task::JoinHandle,
};
use tracing::warn;

pub fn spawn_broadcast_dispatcher(
  mut rec: Receiver<ServerMessage>,
  channel_owner: String,
  mut writer: OwnedWriteHalf,
) -> JoinHandle<Result<(), ApplicationError>> {
  tokio::spawn(async move {
    let success_msg = ServerMessage::Success {
      message: format!("Welcome to the chat 🙏 `{}`", channel_owner),
    };
    let frame = encode_message(&success_msg)?;

    writer.write_all(&frame).await?;
    loop {
      match rec.recv().await {
        Ok(message) => {
          // Skip messages from the channel owner (self-filtering)
          if let Some(message_username) = message.username()
            && message_username == channel_owner
          {
            continue;
          }

          // Handle different message types
          match message {
            ServerMessage::Message { username, content } => {
              let frame = encode_message(&ServerMessage::Message { username, content })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::Error { reason } => {
              let frame = encode_message(&ServerMessage::Error { reason })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::Success { message } => {
              let frame = encode_message(&ServerMessage::Success { message })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::UserJoined { username } => {
              let frame = encode_message(&ServerMessage::UserJoined { username })?;
              writer.write_all(&frame).await?;
            }
            ServerMessage::UserLeft { username } => {
              let frame = encode_message(&ServerMessage::UserLeft { username })?;
              writer.write_all(&frame).await?;
            }
          }
        }
        Err(RecvError::Lagged(lagged_by)) => {
          warn!("reciever lagged by {lagged_by} messages");
        }
        Err(RecvError::Closed) => {
          warn!("Broadcast channel has been closed, Exiting");
          break;
        }
      }
    }

    let success_msg = ServerMessage::Success {
      message: format!("Disconnected: Good bye `{}`!", channel_owner),
    };
    let frame = encode_message(&success_msg)?;
    writer.write_all(&frame).await?;
    Ok(())
  })
}
