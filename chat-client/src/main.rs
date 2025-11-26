use anyhow::Result;
use chat_client::client::ChatClient;
use clap::Parser;
use tracing::Level;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
  #[arg(long, default_value = "127.0.0.1")]
  host: String,

  #[arg(long, default_value_t = 8080)]
  port: u16,

  #[arg(short, long)]
  username: String,
}

#[tokio::main]
async fn main() -> Result<()> {
  tracing_subscriber::fmt::fmt()
    .with_max_level(Level::ERROR)
    .init();

  let args = Args::parse();

  let mut client = ChatClient::new(&args.host, args.port, &args.username).await?;
  client.run().await?;

  Ok(())
}
