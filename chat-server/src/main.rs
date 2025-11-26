use anyhow::Result;
use chat_server::server::ChatServer;
use clap::Parser;
use tokio::signal;
use tokio::sync::oneshot;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
  #[arg(long, default_value = "127.0.0.1")]
  host: String,

  #[arg(long, default_value_t = 8080)]
  port: u16,

  #[arg(long, default_value_t = 1000)]
  max_connections: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt::init();

  let args = Args::parse();

  let server = ChatServer::new(args.max_connections);
  let (shutdown_tx, shutdown_rx) = oneshot::channel();

  let shutdown_signal = shutdown_signal(shutdown_tx);

  let server_task =
    tokio::spawn(async move { server.run(&args.host, args.port, shutdown_rx).await });

  tokio::select! {
    _ = shutdown_signal => {
      tracing::info!("Shutdown signal received, server will shutdown gracefully");
    }
    result = server_task => {
      match result {
        Ok(_) => tracing::info!("Server completed normally"),
        Err(e) => tracing::error!("Server error: {}", e),
      }
    }
  }

  Ok(())
}

async fn shutdown_signal(shutdown_tx: oneshot::Sender<()>) {
  let ctrl_c = async {
    signal::ctrl_c()
      .await
      .expect("failed to install Ctrl+C handler");
  };

  #[cfg(unix)]
  let terminate = async {
    signal::unix::signal(signal::unix::SignalKind::terminate())
      .expect("failed to install signal handler")
      .recv()
      .await;
  };

  #[cfg(not(unix))]
  let terminate = std::future::pending::<()>();

  tokio::select! {
    _ = ctrl_c => {},
    _ = terminate => {},
  }

  tracing::info!("Shutdown signal received, starting graceful shutdown...");

  let _ = shutdown_tx.send(());
}
