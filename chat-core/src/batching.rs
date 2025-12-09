use bytes::Bytes;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, warn};

use crate::error::{ApplicationError, Result};

const DEFAULT_MAX_BATCH_SIZE: usize = 32;
const DEFAULT_FLUSH_INTERVAL_MS: u64 = 5;
const MIN_BATCH_SIZE: usize = 1;
const MAX_BATCH_SIZE: usize = 1000;
const MIN_FLUSH_INTERVAL_MS: u64 = 1;
const MAX_FLUSH_INTERVAL_MS: u64 = 100;

/// Configuration for message batching
#[derive(Debug, Clone, Copy)]
pub struct BatchConfig {
  pub max_batch_size: usize,
  pub flush_interval_ms: u64,
}

impl BatchConfig {
  /// Create a new batch configuration with validation
  pub fn new(max_batch_size: usize, flush_interval_ms: u64) -> Result<Self> {
    let clamped_batch_size = max_batch_size.clamp(MIN_BATCH_SIZE, MAX_BATCH_SIZE);
    let clamped_interval = flush_interval_ms.clamp(MIN_FLUSH_INTERVAL_MS, MAX_FLUSH_INTERVAL_MS);

    if clamped_batch_size != max_batch_size || clamped_interval != flush_interval_ms {
      debug!(
        "BatchConfig adjusted: batch_size {} -> {}, interval {} -> {}",
        max_batch_size, clamped_batch_size, flush_interval_ms, clamped_interval
      );
    }

    Ok(Self {
      max_batch_size: clamped_batch_size,
      flush_interval_ms: clamped_interval,
    })
  }

  /// Validate configuration
  pub fn validate(&self) -> Result<()> {
    if self.max_batch_size < MIN_BATCH_SIZE || self.max_batch_size > MAX_BATCH_SIZE {
      return Err(ApplicationError::config_error(format!(
        "Invalid batch size: {} (must be between {} and {})",
        self.max_batch_size, MIN_BATCH_SIZE, MAX_BATCH_SIZE
      )));
    }

    if self.flush_interval_ms < MIN_FLUSH_INTERVAL_MS
      || self.flush_interval_ms > MAX_FLUSH_INTERVAL_MS
    {
      return Err(ApplicationError::config_error(format!(
        "Invalid flush interval: {}ms (must be between {}ms and {}ms)",
        self.flush_interval_ms, MIN_FLUSH_INTERVAL_MS, MAX_FLUSH_INTERVAL_MS
      )));
    }

    Ok(())
  }
}

impl Default for BatchConfig {
  fn default() -> Self {
    Self {
      max_batch_size: DEFAULT_MAX_BATCH_SIZE,
      flush_interval_ms: DEFAULT_FLUSH_INTERVAL_MS,
    }
  }
}

/// A batch of messages
#[derive(Debug, Clone)]
pub struct MessageBatch {
  messages: Vec<Bytes>,
}

impl MessageBatch {
  pub fn new() -> Self {
    Self {
      messages: Vec::new(),
    }
  }

  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      messages: Vec::with_capacity(capacity),
    }
  }

  pub fn add(&mut self, message: Bytes) {
    self.messages.push(message);
  }

  pub fn is_empty(&self) -> bool {
    self.messages.is_empty()
  }

  pub fn len(&self) -> usize {
    self.messages.len()
  }

  pub fn clear(&mut self) {
    self.messages.clear();
  }

  pub fn messages(&self) -> &[Bytes] {
    &self.messages
  }

  pub fn into_messages(self) -> Vec<Bytes> {
    self.messages
  }
}

impl Default for MessageBatch {
  fn default() -> Self {
    Self::new()
  }
}

/// Batching broadcaster that accumulates messages and flushes periodically
pub struct BatchingBroadcaster {
  pending: Arc<Mutex<MessageBatch>>,
  tx: mpsc::UnboundedSender<MessageBatch>,
  config: BatchConfig,
}

impl BatchingBroadcaster {
  /// Create a new batching broadcaster
  pub fn new(config: BatchConfig) -> Result<(Self, mpsc::UnboundedReceiver<MessageBatch>)> {
    config.validate()?;

    let (tx, rx) = mpsc::unbounded_channel();

    let broadcaster = Self {
      pending: Arc::new(Mutex::new(MessageBatch::with_capacity(
        config.max_batch_size,
      ))),
      tx,
      config,
    };

    Ok((broadcaster, rx))
  }

  /// Add message to pending batch
  pub async fn add_message(&self, message: Bytes) -> Result<()> {
    let should_flush = {
      let mut pending = self.pending.lock().await;
      pending.add(message);
      pending.len() >= self.config.max_batch_size
    };

    if should_flush {
      self.flush_batch().await?;
    }

    Ok(())
  }

  /// Flush current batch
  async fn flush_batch(&self) -> Result<()> {
    let batch = {
      let mut pending = self.pending.lock().await;
      if pending.is_empty() {
        return Ok(());
      }
      std::mem::replace(
        &mut *pending,
        MessageBatch::with_capacity(self.config.max_batch_size),
      )
    };

    self.tx.send(batch).map_err(|e| {
      warn!("Failed to send batch: {}", e);
      ApplicationError::ChannelSendError
    })?;

    Ok(())
  }

  /// Spawn background flush task
  pub fn spawn_flush_task(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
      let mut flush_timer = interval(Duration::from_millis(self.config.flush_interval_ms));
      flush_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);

      loop {
        flush_timer.tick().await;

        match self.flush_batch().await {
          Ok(()) => {}
          Err(ApplicationError::ChannelSendError) => {
            debug!("Flush task: receiver dropped, exiting gracefully");
            break;
          }
          Err(e) => {
            warn!("Flush task error: {}", e);
          }
        }
      }

      debug!("Flush task terminated");
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tokio::time::sleep;

  #[test]
  fn test_batch_config_validation() {
    let valid = BatchConfig::new(50, 10);
    assert!(valid.is_ok());

    let valid_config = valid.expect("Should be valid");
    assert!(valid_config.validate().is_ok());
  }

  #[test]
  fn test_batch_config_clamping() {
    let config = BatchConfig::new(10000, 200).expect("Should clamp values");
    assert_eq!(config.max_batch_size, MAX_BATCH_SIZE);
    assert_eq!(config.flush_interval_ms, MAX_FLUSH_INTERVAL_MS);
  }

  #[tokio::test]
  async fn test_batch_size_trigger() {
    let config = BatchConfig::new(3, 100).expect("Valid config");
    let (broadcaster, mut rx) =
      BatchingBroadcaster::new(config).expect("Failed to create broadcaster");

    broadcaster
      .add_message(Bytes::from("msg1"))
      .await
      .expect("Failed to add");
    broadcaster
      .add_message(Bytes::from("msg2"))
      .await
      .expect("Failed to add");
    broadcaster
      .add_message(Bytes::from("msg3"))
      .await
      .expect("Failed to add");

    let batch = rx.recv().await.expect("Should receive batch");
    assert_eq!(batch.len(), 3);
  }

  #[tokio::test]
  async fn test_time_based_flush() {
    let config = BatchConfig::new(100, 20).expect("Valid config");
    let (broadcaster, mut rx) =
      BatchingBroadcaster::new(config).expect("Failed to create broadcaster");
    let broadcaster = Arc::new(broadcaster);

    let _flush_task = broadcaster.clone().spawn_flush_task();

    broadcaster
      .add_message(Bytes::from("msg1"))
      .await
      .expect("Failed to add");
    broadcaster
      .add_message(Bytes::from("msg2"))
      .await
      .expect("Failed to add");

    sleep(Duration::from_millis(50)).await;

    let batch = rx.recv().await.expect("Should receive batch");
    assert_eq!(batch.len(), 2);
  }
}
