use anipler::{
    config::{DaemonArgs, DaemonConfig},
    daemon::AniplerDaemon,
};
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let args = DaemonArgs::parse();
    let config = DaemonConfig::from_env(&args);
    let daemon = AniplerDaemon::from_config(config).await?;

    daemon.run().await?;

    Ok(())
}
