use crate::config::LoadTestConfig;
use crate::metrics::{ErrorType, LoadTestMetrics, SharedMetrics};
use anyhow::Result;
use chat_core::protocol::{ClientMessage, ServerMessage, encode_message};
use hdrhistogram::Histogram;
use indicatif::{ProgressBar, ProgressStyle};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, mpsc};
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, info, warn};

pub struct LoadTester {
  config: LoadTestConfig,
}

impl LoadTester {
  pub fn new(config: LoadTestConfig) -> Self {
    Self { config }
  }

  pub async fn run(&mut self) -> Result<LoadTestMetrics> {
    let start_time = Instant::now();
    let shared_metrics = SharedMetrics::new(start_time);

    // Create channels for latency measurements
    let (latency_tx, mut latency_rx) = mpsc::unbounded_channel::<u64>();

    // Create progress bar
    let progress = if self.config.total_messages > 0 {
      let pb = ProgressBar::new(self.config.total_messages as u64);
      pb.set_style(
        ProgressStyle::default_bar()
          .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} ({per_sec}) {msg}")
          .unwrap()
          .progress_chars("█▓▒░ "),
      );
      Some(pb)
    } else if self.config.duration_secs > 0 {
      let pb = ProgressBar::new(self.config.duration_secs);
      pb.set_style(
        ProgressStyle::default_bar()
          .template("[{elapsed_precise}] {bar:40.cyan/blue} {pos}s/{len}s {msg}")
          .unwrap()
          .progress_chars("█▓▒░ "),
      );
      Some(pb)
    } else {
      None
    };

    // Global rate limiter using token bucket
    let rate_limiter = if self.config.rate_limit > 0 {
      info!("Enabling rate limiter: {} msg/s", self.config.rate_limit);

      let sem = Arc::new(Semaphore::new(0));
      let sem_clone = Arc::clone(&sem);
      let rate = self.config.rate_limit;

      // Token bucket refill task
      tokio::spawn(async move {
        // Calculate interval in microseconds for precise timing
        let interval_micros = 1_000_000 / rate as u64;
        let mut ticker = interval(Duration::from_micros(interval_micros));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Initial burst - add starting permits
        sem_clone.add_permits(rate.min(10)); // Small initial burst

        loop {
          ticker.tick().await;

          // Don't let permits accumulate too much
          if sem_clone.available_permits() < rate * 2 {
            sem_clone.add_permits(1);
          }
        }
      });

      Some(sem)
    } else {
      None
    };

    // Spawn virtual users
    let mut handles = vec![];
    let rampup_delay = if self.config.rampup_secs > 0 && self.config.connections > 1 {
      Duration::from_secs(self.config.rampup_secs) / (self.config.connections as u32)
    } else {
      Duration::ZERO
    };

    for i in 0..self.config.connections {
      let config = self.config.clone();
      let metrics = shared_metrics.clone();
      let latency_tx = latency_tx.clone();
      let rate_limiter = rate_limiter.clone();
      let progress = progress.clone();

      // Ramp-up delay
      if rampup_delay > Duration::ZERO {
        sleep(rampup_delay).await;
      }

      let handle = tokio::spawn(async move {
        run_virtual_user(i, config, metrics, latency_tx, rate_limiter, progress).await
      });

      handles.push(handle);
    }

    drop(latency_tx);

    // Duration limiter with live stats
    if self.config.duration_secs > 0 {
      let duration = Duration::from_secs(self.config.duration_secs);
      let progress_clone = progress.clone();
      let metrics_clone = shared_metrics.clone();

      tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        let start = Instant::now();

        loop {
          ticker.tick().await;
          let elapsed = start.elapsed().as_secs();

          if elapsed >= duration.as_secs() {
            break;
          }

          if let Some(pb) = &progress_clone {
            let msg_count = metrics_clone
              .messages_sent
              .load(std::sync::atomic::Ordering::Relaxed);
            let rate = if elapsed > 0 {
              msg_count as f64 / elapsed as f64
            } else {
              0.0
            };
            pb.set_position(elapsed);
            pb.set_message(format!("{:.2} msg/s", rate));
          }
        }
      });

      sleep(duration).await;
    }

    // Signal all tasks to stop (they'll finish current operation)
    for handle in handles {
      let _ = handle.await;
    }

    let end_time = Instant::now();

    if let Some(pb) = progress {
      let final_count = shared_metrics
        .messages_sent
        .load(std::sync::atomic::Ordering::Relaxed);
      let final_rate = final_count as f64 / (end_time - start_time).as_secs_f64();
      pb.finish_with_message(format!("Done - {:.2} msg/s", final_rate));
    }

    // Collect latency histogram
    let mut latency_histogram = Histogram::new(3).unwrap();
    while let Ok(latency_micros) = latency_rx.try_recv() {
      let _ = latency_histogram.record(latency_micros);
    }

    // Collect errors
    let errors = shared_metrics.errors.lock().unwrap().clone();

    // Build final metrics
    let mut metrics = LoadTestMetrics {
      start_time,
      end_time,
      duration: end_time - start_time,
      connections_attempted: shared_metrics
        .connections_attempted
        .load(std::sync::atomic::Ordering::Relaxed),
      connections_successful: shared_metrics
        .connections_successful
        .load(std::sync::atomic::Ordering::Relaxed),
      connections_failed: shared_metrics
        .connections_failed
        .load(std::sync::atomic::Ordering::Relaxed),
      messages_sent: shared_metrics
        .messages_sent
        .load(std::sync::atomic::Ordering::Relaxed),
      messages_received: shared_metrics
        .messages_received
        .load(std::sync::atomic::Ordering::Relaxed),
      messages_failed: shared_metrics
        .messages_failed
        .load(std::sync::atomic::Ordering::Relaxed),
      latency_histogram,
      messages_per_second: 0.0,
      bytes_sent: shared_metrics
        .bytes_sent
        .load(std::sync::atomic::Ordering::Relaxed),
      bytes_received: shared_metrics
        .bytes_received
        .load(std::sync::atomic::Ordering::Relaxed),
      errors,
    };

    metrics.calculate_derived_metrics();

    Ok(metrics)
  }
}

async fn run_virtual_user(
  user_id: usize,
  config: LoadTestConfig,
  metrics: SharedMetrics,
  latency_tx: mpsc::UnboundedSender<u64>,
  rate_limiter: Option<Arc<Semaphore>>,
  progress: Option<ProgressBar>,
) {
  let username = format!("{}{}", config.username_prefix, user_id);

  // Connect and authenticate
  metrics.increment_connections_attempted();

  let stream = match timeout(
    Duration::from_secs(config.timeout_secs),
    TcpStream::connect(config.server_address()),
  )
  .await
  {
    Ok(Ok(stream)) => stream,
    Ok(Err(e)) => {
      warn!("User {} failed to connect: {}", username, e);
      metrics.record_error(username.clone(), ErrorType::ConnectionFailed, e.to_string());
      metrics.increment_connections_failed();
      return;
    }
    Err(_) => {
      warn!("User {} connection timeout", username);
      metrics.record_error(
        username.clone(),
        ErrorType::Timeout,
        "Connection timeout".to_string(),
      );
      metrics.increment_connections_failed();
      return;
    }
  };

  let (mut reader, mut writer) = stream.into_split();

  // Send join message
  let join_msg = ClientMessage::Join {
    username: username.clone(),
  };

  let join_frame = match encode_message(&join_msg) {
    Ok(f) => f,
    Err(e) => {
      warn!("User {} failed to encode join: {}", username, e);
      metrics.record_error(
        username.clone(),
        ErrorType::Other,
        format!("Encode error: {}", e),
      );
      metrics.increment_connections_failed();
      return;
    }
  };

  metrics.add_bytes_sent(join_frame.len() as u64);

  if writer.write_all(&join_frame).await.is_err() || writer.flush().await.is_err() {
    warn!("User {} failed to send join", username);
    metrics.record_error(
      username.clone(),
      ErrorType::SendFailed,
      "Failed to send join".to_string(),
    );
    metrics.increment_connections_failed();
    return;
  }

  // Read welcome message
  match read_server_message(&mut reader, &metrics).await {
    Ok(ServerMessage::Success { .. }) => {
      metrics.increment_connections_successful();
      debug!("User {} connected successfully", username);
    }
    Ok(ServerMessage::Error { reason }) => {
      warn!("User {} join rejected: {}", username, reason);
      metrics.record_error(username.clone(), ErrorType::AuthenticationFailed, reason);
      metrics.increment_connections_failed();
      return;
    }
    Ok(other) => {
      let msg = format!("Unexpected response: {:?}", other);
      warn!("User {} got unexpected response: {:?}", username, other);
      metrics.record_error(username.clone(), ErrorType::Other, msg);
      metrics.increment_connections_failed();
      return;
    }
    Err(e) => {
      warn!("User {} failed to read welcome: {}", username, e);
      metrics.record_error(username.clone(), ErrorType::ReceiveFailed, e.to_string());
      metrics.increment_connections_failed();
      return;
    }
  }

  // Spawn background reader
  let metrics_clone = metrics.clone();
  let username_clone = username.clone();
  tokio::spawn(async move {
    while read_server_message(&mut reader, &metrics_clone)
      .await
      .is_ok()
    {}
    debug!("User {} reader stopped", username_clone);
  });

  // Message sending loop
  let mut message_count = 0;
  let max_messages = if config.total_messages > 0 {
    config.total_messages / config.connections
  } else {
    usize::MAX
  };

  // Use think time if no global rate limiter
  let think_interval = if rate_limiter.is_none() && config.think_time_ms > 0 {
    Some(Duration::from_millis(config.think_time_ms))
  } else {
    None
  };

  loop {
    if message_count >= max_messages {
      break;
    }

    // CRITICAL: Wait for rate limit permit BEFORE doing anything
    if let Some(sem) = &rate_limiter {
      // Block here until we get a permit
      match sem.acquire().await {
        Ok(permit) => {
          permit.forget(); // Consume the permit
        }
        Err(_) => {
          debug!("User {} rate limiter closed", username);
          break;
        }
      }
    } else if let Some(interval) = think_interval {
      // Use think time instead
      sleep(interval).await;
    }

    // NOW send the message
    let content = if config.random_messages {
      format!("Random {} from {}", rand::random::<u32>(), username)
    } else {
      config
        .message_template
        .replace("{user}", &username)
        .replace("{count}", &message_count.to_string())
    };

    let chat_msg = ClientMessage::Message {
      username: username.clone(),
      content,
    };

    let frame = match encode_message(&chat_msg) {
      Ok(f) => f,
      Err(e) => {
        metrics.record_error(username.clone(), ErrorType::Other, format!("Encode: {}", e));
        metrics.increment_messages_failed();
        continue;
      }
    };

    let send_start = Instant::now();

    if writer.write_all(&frame).await.is_err() || writer.flush().await.is_err() {
      metrics.increment_messages_failed();
      break;
    }

    metrics.add_bytes_sent(frame.len() as u64);
    metrics.increment_messages_sent();
    message_count += 1;

    let latency_micros = send_start.elapsed().as_micros() as u64;
    let _ = latency_tx.send(latency_micros);

    if let Some(pb) = &progress {
      pb.inc(1);
    }
  }

  debug!("User {} sent {} messages", username, message_count);

  // Leave
  let leave_msg = ClientMessage::Leave {
    username: username.clone(),
  };
  if let Ok(frame) = encode_message(&leave_msg) {
    let _ = writer.write_all(&frame).await;
    let _ = writer.flush().await;
  }
}

async fn read_server_message(
  reader: &mut tokio::net::tcp::OwnedReadHalf,
  metrics: &SharedMetrics,
) -> Result<ServerMessage> {
  let mut length_bytes = [0u8; 4];
  reader.read_exact(&mut length_bytes).await?;
  let payload_len = u32::from_be_bytes(length_bytes) as usize;

  let mut payload = vec![0u8; payload_len];
  reader.read_exact(&mut payload).await?;

  metrics.add_bytes_received((4 + payload_len) as u64);
  metrics.increment_messages_received();

  let (msg, _) = bincode::decode_from_slice(&payload, bincode::config::standard())
    .map_err(|e| anyhow::anyhow!("Decode: {}", e))?;

  Ok(msg)
}
