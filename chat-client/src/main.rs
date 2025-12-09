mod client;

use chat_core::error::Result;
use clap::Parser;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt};

const DEFAULT_HOST: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 8080;

#[derive(Parser, Debug)]
#[command(name = "chat-client")]
#[command(version, about = "High-performance async chat client", long_about = None)]
struct Args {
  /// Server host address
  #[arg(short = 'H', long, default_value = DEFAULT_HOST)]
  host: String,

  /// Server port
  #[arg(short, long, default_value_t = DEFAULT_PORT)]
  port: u16,

  /// Your username
  #[arg(short, long)]
  username: String,
}

impl Args {
  fn validate(&self) -> Result<()> {
    if self.port == 0 {
      return Err(chat_core::error::ApplicationError::config_error(
        "Port cannot be 0".to_string(),
      ));
    }

    chat_core::utils::validate_username(&self.username)?;

    Ok(())
  }
}

#[tokio::main]
async fn main() -> Result<()> {
  // Initialize tracing
  let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

  fmt().with_env_filter(filter).with_target(false).init();

  let args = Args::parse();

  // Validate arguments
  if let Err(e) = args.validate() {
    error!("Invalid configuration: {}", e);
    std::process::exit(1);
  }

  info!("Connecting to chat server at {}:{}", args.host, args.port);
  info!("Username: {}", args.username);

  // Run client
  match client::run_client(&args.host, args.port, &args.username).await {
    Ok(()) => {
      info!("Client terminated successfully");
      Ok(())
    }
    Err(e) => {
      error!("Client error: {}", e);
      Err(e)
    }
  }
}
