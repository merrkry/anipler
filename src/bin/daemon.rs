use anipler::{
    config::{DaemonArgs, DaemonConfig},
    daemon::AniplerDaemon,
};
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let init_span = tracing::span!(tracing::Level::INFO, "daemon_init").entered();

    let args = DaemonArgs::parse();
    let config = DaemonConfig::load(&args)?;
    tracing::trace!("Loaded daemon configuration");

    let daemon = AniplerDaemon::from_config(config).await?;
    tracing::info!("Daemon initialized successfully");
    drop(init_span);

    daemon.run().await?;

    tracing::info!("Daemon stopped");
    Ok(())
}
