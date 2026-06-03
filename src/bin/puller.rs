use anipler::{
    config::{PullerArgs, PullerConfig},
    puller::AniplerPuller,
};

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() -> Result<()> {
    let args = PullerArgs::parse();
    // CLI arg > env var > default
    let env_filter = match args.log_level {
        Some(ref l) => l.parse()?,
        None => EnvFilter::builder()
            .with_default_directive(LevelFilter::INFO.into())
            .from_env()?,
    };
    fmt().with_env_filter(env_filter).init();

    let config = PullerConfig::load(&args)?;

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
