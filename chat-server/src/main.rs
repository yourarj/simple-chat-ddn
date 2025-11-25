// server/src/main.rs
use anyhow::Result;
use chat_server::server::ChatServer;
use clap::Parser;

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
  server.run(&args.host, args.port).await?;

  Ok(())
}
