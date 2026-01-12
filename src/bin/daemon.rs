use anipler::config::DaemonConfig;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let config = DaemonConfig::from_env();

    Ok(())
}
