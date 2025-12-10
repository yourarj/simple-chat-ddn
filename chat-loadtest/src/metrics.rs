use hdrhistogram::Histogram;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct LoadTestMetrics {
  pub start_time: Instant,
  pub end_time: Instant,
  pub duration: Duration,

  // Connection metrics
  pub connections_attempted: usize,
  pub connections_successful: usize,
  pub connections_failed: usize,

  // Message metrics
  pub messages_sent: usize,
  pub messages_received: usize,
  pub messages_failed: usize,

  // Latency metrics (microseconds)
  pub latency_histogram: Histogram<u64>,

  // Throughput
  pub messages_per_second: f64,
  pub bytes_sent: u64,
  pub bytes_received: u64,

  // Errors
  pub errors: Vec<ErrorRecord>,
}

#[derive(Debug, Clone)]
pub struct ErrorRecord {
  pub timestamp: Duration,
  pub user: String,
  pub error_type: ErrorType,
  pub message: String,
}

#[derive(Debug, Clone)]
pub enum ErrorType {
  ConnectionFailed,
  AuthenticationFailed,
  SendFailed,
  ReceiveFailed,
  Timeout,
  Other,
}

impl LoadTestMetrics {
  pub fn calculate_derived_metrics(&mut self) {
    self.duration = self.end_time - self.start_time;
    let duration_secs = self.duration.as_secs_f64();

    if duration_secs > 0.0 {
      self.messages_per_second = self.messages_sent as f64 / duration_secs;
    }
  }

  pub fn get_percentile(&self, percentile: f64) -> Duration {
    let micros = self.latency_histogram.value_at_percentile(percentile);
    Duration::from_micros(micros)
  }

  pub fn get_min_latency(&self) -> Duration {
    Duration::from_micros(self.latency_histogram.min())
  }

  pub fn get_max_latency(&self) -> Duration {
    Duration::from_micros(self.latency_histogram.max())
  }

  pub fn get_mean_latency(&self) -> Duration {
    Duration::from_micros(self.latency_histogram.mean() as u64)
  }

  pub fn get_stddev_latency(&self) -> Duration {
    Duration::from_micros(self.latency_histogram.stdev() as u64)
  }

  pub fn get_error_summary(&self) -> Vec<(ErrorType, usize)> {
    let mut summary = std::collections::HashMap::new();

    for error in &self.errors {
      *summary
        .entry(format!("{:?}", error.error_type))
        .or_insert(0) += 1;
    }

    summary
      .into_iter()
      .map(|(k, v)| {
        let error_type = match k.as_str() {
          "ConnectionFailed" => ErrorType::ConnectionFailed,
          "AuthenticationFailed" => ErrorType::AuthenticationFailed,
          "SendFailed" => ErrorType::SendFailed,
          "ReceiveFailed" => ErrorType::ReceiveFailed,
          "Timeout" => ErrorType::Timeout,
          _ => ErrorType::Other,
        };
        (error_type, v)
      })
      .collect()
  }
}

#[derive(Clone)]
pub struct SharedMetrics {
  pub connections_attempted: Arc<AtomicUsize>,
  pub connections_successful: Arc<AtomicUsize>,
  pub connections_failed: Arc<AtomicUsize>,
  pub messages_sent: Arc<AtomicUsize>,
  pub messages_received: Arc<AtomicUsize>,
  pub messages_failed: Arc<AtomicUsize>,
  pub bytes_sent: Arc<AtomicU64>,
  pub bytes_received: Arc<AtomicU64>,
  pub errors: Arc<Mutex<Vec<ErrorRecord>>>,
  pub start_time: Instant,
}

impl SharedMetrics {
  pub fn new(start_time: Instant) -> Self {
    Self {
      connections_attempted: Arc::new(AtomicUsize::new(0)),
      connections_successful: Arc::new(AtomicUsize::new(0)),
      connections_failed: Arc::new(AtomicUsize::new(0)),
      messages_sent: Arc::new(AtomicUsize::new(0)),
      messages_received: Arc::new(AtomicUsize::new(0)),
      messages_failed: Arc::new(AtomicUsize::new(0)),
      bytes_sent: Arc::new(AtomicU64::new(0)),
      bytes_received: Arc::new(AtomicU64::new(0)),
      errors: Arc::new(Mutex::new(Vec::new())),
      start_time,
    }
  }

  pub fn increment_connections_attempted(&self) {
    self.connections_attempted.fetch_add(1, Ordering::Relaxed);
  }

  pub fn increment_connections_successful(&self) {
    self.connections_successful.fetch_add(1, Ordering::Relaxed);
  }

  pub fn increment_connections_failed(&self) {
    self.connections_failed.fetch_add(1, Ordering::Relaxed);
  }

  pub fn increment_messages_sent(&self) {
    self.messages_sent.fetch_add(1, Ordering::Relaxed);
  }

  pub fn increment_messages_received(&self) {
    self.messages_received.fetch_add(1, Ordering::Relaxed);
  }

  pub fn increment_messages_failed(&self) {
    self.messages_failed.fetch_add(1, Ordering::Relaxed);
  }

  pub fn add_bytes_sent(&self, bytes: u64) {
    self.bytes_sent.fetch_add(bytes, Ordering::Relaxed);
  }

  pub fn add_bytes_received(&self, bytes: u64) {
    self.bytes_received.fetch_add(bytes, Ordering::Relaxed);
  }

  pub fn record_error(&self, user: String, error_type: ErrorType, message: String) {
    let elapsed = self.start_time.elapsed();
    let error = ErrorRecord {
      timestamp: elapsed,
      user,
      error_type,
      message,
    };

    if let Ok(mut errors) = self.errors.lock() {
      errors.push(error);
    }
  }
}
