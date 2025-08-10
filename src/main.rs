use anyhow::Result;
use std::env;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Nabla Enterprise Runner Server");

    // Get port from environment or default to 8080
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    // Run the Axum server
    nabla_runner::server::run_server(port).await?;

    Ok(())
}