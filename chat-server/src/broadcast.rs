use chat_core::{
  error::ApplicationError,
  protocol::{ChatMessage, encode_message},
};
use tokio::{
  io::AsyncWriteExt,
  net::tcp::OwnedWriteHalf,
  sync::broadcast::{Receiver, error::RecvError},
  task::JoinHandle,
};
use tracing::warn;

pub fn spawn_broadcast_dispatcher(
  mut rec: Receiver<ChatMessage>,
  channel_owner: String,
  mut writer: OwnedWriteHalf,
) -> JoinHandle<Result<(), ApplicationError>> {
  tokio::spawn(async move {
    let success_msg = ChatMessage::Success {
      message: format!("Welcome to the chat 🙏 `{}`", channel_owner),
    };
    let frame = encode_message(&success_msg)?;

    writer.write_all(&frame).await?;
    loop {
      match rec.recv().await {
        Ok(message) => {
          if Some(channel_owner.as_str()) == message.username() {
            continue;
          }

          match message {
            ChatMessage::Join { username: _ } => todo!(),
            ChatMessage::Leave { username: _ } => todo!(),
            ChatMessage::Message { username, content } => {
              let cont = encode_message(&ChatMessage::Message { username, content })?;
              writer.write_all(&cont).await?;
            }
            ChatMessage::Error { reason: _ } => todo!(),
            ChatMessage::Success { message: _ } => todo!(),
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

    let success_msg = ChatMessage::Success {
      message: "Disconnected: Good bye `{user}`!".to_string(),
    };
    let frame = encode_message(&success_msg)?;
    writer.write_all(&frame).await?;
    Ok(())
  })
}
