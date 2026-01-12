use anipler::{config::DaemonConfig, daemon::AniplerDaemon};
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config = DaemonConfig::from_env();
    let daemon = AniplerDaemon::from_config(config).await?;

    daemon.run().await?;

    Ok(())
}
