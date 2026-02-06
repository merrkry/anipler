use anipler::puller::{self, AniplerPuller, Args, PullerConfig};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use std::{env, path::PathBuf};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let log_level = args
        .log_level
        .clone()
        .or_else(|| env::var("RUST_LOG").ok())
        .unwrap_or_else(|| "info".to_string());
    fmt()
        .with_max_level(log_level.parse::<LevelFilter>()?)
        .init();

    let config_path = env::var("ANIPLER_CONFIG_PATH")
        .ok()
        .map(PathBuf::from)
        .or(args.config)
        .or_else(|| puller::default_config_path().ok())
        .ok_or_else(|| anyhow!("Failed to determine config path"))?;

    let config = PullerConfig::from_path(&config_path)
        .with_context(|| format!("Failed to read config from {}", config_path.display()))?;

    tracing::debug!("Loaded puller configuration from {}", config_path.display());

    let client = AniplerPuller::from_config(config);

    let artifacts_count = client.fetch_artifacts_list().await?;
    tracing::info!("Found {} artifacts to pull", artifacts_count);

    loop {
        match client.transfer_next().await {
            Ok(Some(())) => {}
            Ok(None) => {
                tracing::info!("All artifacts transferred successfully");
                break;
            }
            Err(e) => {
                tracing::error!(error = ?e, "Error occurred during artifact transfer, aborting");
                break;
            }
        }
    }

    Ok(())
}
