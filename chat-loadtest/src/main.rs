mod config;
mod loader;
mod metrics;
mod reporter;

use anyhow::Result;
use clap::Parser;
use colored::Colorize;
use config::LoadTestConfig;
use loader::LoadTester;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "chat-load-test")]
#[command(version, about = "Load testing tool for chat server", long_about = None)]
struct Args {
  /// Server host address
  #[arg(short = 'H', long, default_value = "127.0.0.1")]
  host: String,

  /// Server port
  #[arg(short = 'p', long, default_value_t = 8080)]
  port: u16,

  /// Number of concurrent connections (virtual users)
  #[arg(short = 'u', long = "users", default_value_t = 10)]
  connections: usize,

  /// Session duration in seconds
  #[arg(long = "session-duration", default_value_t = 30)]
  duration: u64,

  /// Total number of messages to send (0 = unlimited)
  #[arg(short = 'n', long = "messages", default_value_t = 0)]
  messages: usize,

  /// Messages per second rate limit (0 = unlimited)
  #[arg(long = "messages-per-second", default_value_t = 0)]
  rate: usize,

  /// Load test mode: throughput, latency, or stress
  #[arg(long = "mode", default_value = "throughput")]
  mode: String,

  /// Connection ramp-up time in seconds (0 = connect all at once)
  #[arg(short = 'R', long = "rampup", default_value_t = 0)]
  rampup: u64,

  /// Think time between messages in milliseconds
  #[arg(short = 't', long = "think-time", default_value_t = 100)]
  think_time: u64,

  /// Connection timeout in seconds
  #[arg(long = "timeout", default_value_t = 10)]
  timeout: u64,

  /// Username prefix for virtual users
  #[arg(long = "username-prefix", default_value = "user")]
  username_prefix: String,

  /// Message content template (use {user} and {count} as placeholders)
  #[arg(
    long = "message-template",
    default_value = "Message {count} from {user}"
  )]
  message_template: String,

  /// Warmup duration in seconds (excluded from results)
  #[arg(short = 'w', long = "warmup", default_value_t = 0)]
  warmup: u64,

  /// Report format: text, json, csv
  #[arg(long = "format", default_value = "text")]
  format: String,

  /// Output file for report (default: stdout)
  #[arg(short = 'o', long = "output")]
  output: Option<String>,

  /// Enable verbose logging
  #[arg(short = 'v', long = "verbose")]
  verbose: bool,

  /// Random message content (ignore template)
  #[arg(long = "random-messages")]
  random_messages: bool,

  /// Percentiles to calculate (comma-separated, e.g., "50,90,95,99")
  #[arg(long = "percentiles", default_value = "50,90,95,99,99.9")]
  percentiles: String,
}

#[tokio::main]
async fn main() -> Result<()> {
  let args = Args::parse();

  // Initialize logging
  let log_level = if args.verbose { "debug" } else { "info" };
  tracing_subscriber::fmt()
    .with_max_level(
      log_level
        .parse::<tracing_subscriber::filter::LevelFilter>()
        .unwrap(),
    )
    .with_target(false)
    .init();

  // Parse percentiles
  let percentiles: Vec<f64> = args
    .percentiles
    .split(',')
    .filter_map(|s| s.trim().parse().ok())
    .collect();

  // Build configuration
  let config = LoadTestConfig {
    host: args.host,
    port: args.port,
    connections: args.connections,
    duration_secs: args.duration,
    total_messages: args.messages,
    rate_limit: args.rate,
    rampup_secs: args.rampup,
    think_time_ms: args.think_time,
    timeout_secs: args.timeout,
    username_prefix: args.username_prefix,
    message_template: args.message_template,
    warmup_secs: args.warmup,
    random_messages: args.random_messages,
    percentiles,
  };

  // Validate configuration
  config.validate()?;

  // Print banner
  print_banner(&config);

  // Run load test
  info!("Starting load test...");
  let mut loader = LoadTester::new(config);

  match loader.run().await {
    Ok(metrics) => {
      info!("Load test completed successfully");

      // Print results
      match args.format.as_str() {
        "json" => reporter::print_json(&metrics, args.output)?,
        "csv" => reporter::print_csv(&metrics, args.output)?,
        _ => reporter::print_text(&metrics, args.output)?,
      }

      Ok(())
    }
    Err(e) => {
      error!("Load test failed: {}", e);
      Err(e)
    }
  }
}

fn print_banner(config: &LoadTestConfig) {
  println!(
    "\n{}",
    "╔═══════════════════════════════════════════════════════════╗".cyan()
  );
  println!(
    "{}",
    "║           Chat Server Load Testing Tool                  ║".cyan()
  );
  println!(
    "{}",
    "╚═══════════════════════════════════════════════════════════╝".cyan()
  );
  println!();
  println!("  {}  {}:{}", "Target:".bold(), config.host, config.port);
  println!("  {}  {}", "Connections:".bold(), config.connections);

  if config.duration_secs > 0 {
    println!("  {}  {}s", "Duration:".bold(), config.duration_secs);
  }

  if config.total_messages > 0 {
    println!("  {}  {}", "Total Messages:".bold(), config.total_messages);
  }

  if config.rate_limit > 0 {
    println!("  {}  {} msg/s", "Rate Limit:".bold(), config.rate_limit);
  } else {
    println!(
      "  {}  {}",
      "Rate Limit:".bold().red(),
      "UNLIMITED (max speed)".red()
    );
  }

  if config.rampup_secs > 0 {
    println!("  {}  {}s", "Ramp-up:".bold(), config.rampup_secs);
  }

  if config.warmup_secs > 0 {
    println!("  {}  {}s", "Warmup:".bold(), config.warmup_secs);
  }

  println!("  {}  {}ms", "Think Time:".bold(), config.think_time_ms);
  println!();
}
