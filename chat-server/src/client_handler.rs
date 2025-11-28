use chat_core::{
  error::ApplicationError,
  message_cahce::MessageCache,
  protocol::{ClientMessage, ServerMessage, SharedServerMessage, encode_message},
  transport_layer::read_message_from_stream,
  utils,
};
use tokio::{io::AsyncWriteExt, net::TcpStream};
use tracing::info;

use crate::broadcast_pool::BroadcastPool;

pub(super) async fn handle_client(
  stream: TcpStream,
  broadcast_pool: BroadcastPool,
  cache: MessageCache,
) -> Result<(), ApplicationError> {
  let (mut reader, mut writer) = stream.into_split();

  let mut buffer = vec![0u8; 4096];
  let username: Option<String>;

  loop {
    match read_message_from_stream(&mut reader, &mut buffer).await {
      Ok(ClientMessage::Join { username: un }) => {
        // validate username
        utils::validate_username(&un)?;
        if broadcast_pool.has(&un) {
          let error_msg = ServerMessage::user_name_already_taken(un.to_string());
          let frame = encode_message(&error_msg)?;
          writer.write_all(&frame).await?;
          continue;
        }
        username = Some(un);
        break;
      }
      Ok(_) => {
        let error_msg = ServerMessage::Error {
          reason: "You have to join the group before doing any other operation".to_string(),
        };
        let frame = encode_message(&error_msg)?;
        writer.write_all(&frame).await?;
      }
      Err(_) => (),
    }
  }

  let user = username.ok_or(ApplicationError::UsernameNotFound)?;

  broadcast_pool.create_dispatcher(user.clone(), writer, cache);

  let join_notification = SharedServerMessage::new(ServerMessage::user_joined(user.clone()));
  broadcast_pool.broadcast_message(join_notification).await?;

  loop {
    tokio::select! {
      message_result = read_message_from_stream(&mut reader, &mut buffer) => {
        match message_result {
          Ok(ClientMessage::Join { username: _ }) => {}
          Ok(ClientMessage::Leave { username: user }) => {
            broadcast_pool.destroy_dispatcher(&user);
            info!("User {} left the chat", user);

            let msg = SharedServerMessage::new(ServerMessage::user_left(user.clone()));
            broadcast_pool.broadcast_message(msg).await?;
            break;
          }
          Ok(ClientMessage::Message {
            username: user,
            content,
          }) => {
            let msg = SharedServerMessage::new(ServerMessage::message(user.clone(), content.clone()));
            broadcast_pool.broadcast_message(msg).await?;
            info!("Message from {}: {}", user, content);
          }
          Err(ApplicationError::ClientReadStreamClosed) => {
            info!("Client disconnected");
            break;
          }
          Err(_) => {

            break;
          }
        }
      }
    }
  }

  broadcast_pool.destroy_dispatcher(&user);

  Ok(())
}
