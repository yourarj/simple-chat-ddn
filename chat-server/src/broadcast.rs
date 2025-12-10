use chat_core::{
  message_cache::MessageCache,
  protocol::{ServerMessage, SharedServerMessage, UserMessageCounter, encode_message_with_cache},
  transport_layer::BatchedWriter,
};
use std::sync::Arc;
use tokio::{
  net::tcp::OwnedWriteHalf,
  sync::broadcast::{Receiver, error::RecvError},
  task::JoinHandle,
  time::{MissedTickBehavior, interval},
};
use tracing::{debug, info, warn};

/// Spawn a broadcast dispatcher task for a user
///
/// # Key Changes for Username + Counter:
/// 1. Added `counter: Arc<UserMessageCounter>` parameter
/// 2. Use `SharedServerMessage::new_with_counter()` for user messages
/// 3. Use `SharedServerMessage::new_system()` for system messages
/// 4. Log message count on exit using `counter.current_count()`
pub fn spawn_broadcast_dispatcher(
  mut receiver: Receiver<SharedServerMessage>,
  username: String,
  writer: OwnedWriteHalf,
  cache: MessageCache,
  counter: Arc<UserMessageCounter>, // ✅ NEW: User's message counter
) -> JoinHandle<()> {
  tokio::spawn(async move {
    let mut batched_writer = BatchedWriter::new(writer);
    let flush_interval = batched_writer.flush_interval();
    let mut flush_timer = interval(flush_interval);
    flush_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

    debug!("Broadcast dispatcher started for user: {}", username);

    // ✅ CHANGED: Send welcome message using user's counter
    let welcome_msg = SharedServerMessage::new_with_counter(
      ServerMessage::success(format!("Welcome to the chat, {}! 🎉", username)),
      &counter, // Uses counter for unique message ID
    );

    match encode_message_with_cache(&welcome_msg, &cache) {
      Ok(frame) => {
        batched_writer.add_message(&frame);
        if let Err(e) = batched_writer.flush().await {
          warn!("Failed to send welcome message to {}: {}", username, e);
          return;
        }
      }
      Err(e) => {
        warn!("Failed to encode welcome message: {}", e);
        return;
      }
    }

    info!("User '{}' joined the chat", username);

    // Message forwarding loop
    loop {
      tokio::select! {
          recv_result = receiver.recv() => {
              match recv_result {
                  Ok(message) => {
                      // Skip own messages
                      if let Some(msg_username) = message.username()
                          && msg_username == username {
                              continue;
                          }

                      // ✅ UNCHANGED: Encode uses message's embedded ID (from counter)
                      match encode_message_with_cache(&message, &cache) {
                          Ok(frame) => {
                              batched_writer.add_message(&frame);

                              if batched_writer.should_flush()
                                  && let Err(e) = batched_writer.flush().await {
                                      warn!("Write error for user '{}': {}", username, e);
                                      break;
                                  }
                          }
                          Err(e) => {
                              warn!("Failed to encode message: {}", e);
                          }
                      }
                  }
                  Err(RecvError::Lagged(lagged_by)) => {
                      warn!("User '{}' lagged by {} messages", username, lagged_by);

                      // ✅ CHANGED: Use system message for lag notification
                      let lag_msg = SharedServerMessage::new_system(ServerMessage::error(
                          format!("You missed {} messages due to slow connection", lagged_by),
                      ));

                      if let Ok(frame) = encode_message_with_cache(&lag_msg, &cache) {
                          batched_writer.add_message(&frame);
                          let _ = batched_writer.flush().await;
                      }
                  }
                  Err(RecvError::Closed) => {
                      debug!("Broadcast channel closed for user '{}'", username);
                      break;
                  }
              }
          }

          _ = flush_timer.tick() => {
              if let Err(e) = batched_writer.flush().await {
                  warn!("Flush error for user '{}': {}", username, e);
                  break;
              }
          }
      }
    }

    // ✅ CHANGED: Send goodbye message using user's counter
    let goodbye_msg = SharedServerMessage::new_with_counter(
      ServerMessage::success(format!("Goodbye, {}! 👋", username)),
      &counter,
    );
    if let Ok(frame) = encode_message_with_cache(&goodbye_msg, &cache) {
      batched_writer.add_message(&frame);
      let _ = batched_writer.flush().await;
    }

    if let Err(e) = batched_writer.shutdown().await {
      warn!("Error during writer shutdown for '{}': {}", username, e);
    }

    // ✅ NEW: Log total message count for analytics
    info!(
      "User '{}' left the chat (total messages: {})",
      username,
      counter.current_count()
    );
  })
}
