use anyhow::{Result, bail};

#[derive(Debug, Clone)]
pub struct LoadTestConfig {
  pub host: String,
  pub port: u16,
  pub connections: usize,
  pub duration_secs: u64,
  pub total_messages: usize,
  pub rate_limit: usize,
  pub rampup_secs: u64,
  pub think_time_ms: u64,
  pub timeout_secs: u64,
  pub username_prefix: String,
  pub message_template: String,
  pub warmup_secs: u64,
  pub random_messages: bool,
  pub percentiles: Vec<f64>,
}

impl LoadTestConfig {
  pub fn validate(&self) -> Result<()> {
    if self.connections == 0 {
      bail!("Connections must be at least 1");
    }

    if self.connections > 100_000 {
      bail!("Connections cannot exceed 100,000");
    }

    if self.duration_secs == 0 && self.total_messages == 0 {
      bail!("Either duration or total messages must be specified");
    }

    if self.port == 0 {
      bail!("Invalid port number");
    }

    if self.percentiles.iter().any(|&p| p <= 0.0 || p > 100.0) {
      bail!("Percentiles must be between 0 and 100");
    }

    Ok(())
  }

  pub fn server_address(&self) -> String {
    format!("{}:{}", self.host, self.port)
  }
}
