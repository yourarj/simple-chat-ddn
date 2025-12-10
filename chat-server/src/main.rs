mod broadcast;
mod broadcast_pool;
mod client_handler;
mod server;

use chat_core::error::Result;
use clap::Parser;
use server::ChatServer;
use tracing::{error, info};
use tracing_subscriber::{filter::EnvFilter, fmt};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;
const DEFAULT_MAX_CONNECTIONS: usize = 10_000;
const DEFAULT_CACHE_CAPACITY: usize = 10_000;
const DEFAULT_BROADCAST_CAPACITY: usize = 100_000;

#[derive(Parser, Debug)]
#[command(name = "chat-server")]
#[command(version, about = "High-concurrency async chat server", long_about = None)]
struct Args {
  /// Server bind address
  #[arg(short = 'H', long, default_value = DEFAULT_HOST)]
  host: String,

  /// Server bind port
  #[arg(short, long, default_value_t = DEFAULT_PORT)]
  port: u16,

  /// Maximum concurrent connections
  #[arg(short, long, default_value_t = DEFAULT_MAX_CONNECTIONS)]
  max_connections: usize,

  /// Message cache capacity
  #[arg(short, long, default_value_t = DEFAULT_CACHE_CAPACITY)]
  cache_capacity: usize,

  /// Broadcast channel capacity
  #[arg(short, long, default_value_t = DEFAULT_BROADCAST_CAPACITY)]
  broadcast_capacity: usize,
}

impl Args {
  fn validate(&self) -> Result<()> {
    if self.port == 0 {
      return Err(chat_core::error::ApplicationError::config_error(
        "Port cannot be 0".to_string(),
      ));
    }

    if self.max_connections == 0 {
      return Err(chat_core::error::ApplicationError::config_error(
        "Max connections must be at least 1".to_string(),
      ));
    }

    if self.cache_capacity == 0 {
      return Err(chat_core::error::ApplicationError::config_error(
        "Cache capacity must be at least 1".to_string(),
      ));
    }

    if self.broadcast_capacity == 0 {
      return Err(chat_core::error::ApplicationError::config_error(
        "Broadcast capacity must be at least 1".to_string(),
      ));
    }

    Ok(())
  }
}

#[tokio::main]
async fn main() -> Result<()> {
  // Initialize tracing with env filter
  let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

  fmt().with_env_filter(filter).with_target(false).init();

  let args = Args::parse();

  // Validate arguments
  if let Err(e) = args.validate() {
    error!("Invalid configuration: {}", e);
    std::process::exit(1);
  }

  info!("Starting chat server...");
  info!("Configuration:");
  info!("  Host: {}", args.host);
  info!("  Port: {}", args.port);
  info!("  Max connections: {}", args.max_connections);
  info!("  Cache capacity: {}", args.cache_capacity);
  info!("  Broadcast capacity: {}", args.broadcast_capacity);

  // Create server
  let server = ChatServer::new(
    args.max_connections,
    args.cache_capacity,
    args.broadcast_capacity,
  );

  // Setup graceful shutdown
  let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

  // Spawn CTRL+C handler
  tokio::spawn(async move {
    match tokio::signal::ctrl_c().await {
      Ok(()) => {
        info!("Received CTRL+C, initiating shutdown...");
        let _ = shutdown_tx.send(());
      }
      Err(e) => {
        error!("Failed to listen for CTRL+C: {}", e);
      }
    }
  });

  // Run server
  match server.run(&args.host, args.port, shutdown_rx).await {
    Ok(()) => {
      info!("Server shutdown complete");
      Ok(())
    }
    Err(e) => {
      error!("Server error: {}", e);
      Err(e)
    }
  }
}
