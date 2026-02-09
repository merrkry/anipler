use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use shell_words;
use tap::Pipe;
use tokio::{process::Command, sync::Mutex};
use url::Url;

use crate::task::ArtifactInfo;

#[derive(Debug, Deserialize)]
pub struct PullerConfig {
    pub api_url: Url,
    pub api_key: String,
    pub ssh_host: String,
    #[serde(default = "get_cwd")]
    pub destination: PathBuf,
}

fn get_cwd() -> PathBuf {
    std::env::current_dir().expect("Failed to get current working directory")
}

impl PullerConfig {
    /// Load the puller configuration from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or parsed.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut config: Self =
            toml::from_str(&content).map_err(|e| anyhow::anyhow!("Failed to parse config: {e}"))?;

        config.destination = config
            .destination
            .to_string_lossy()
            .pipe(|s| {
                shellexpand::full(&s)
                    .map(String::from)
                    .map_err(|e| anyhow::anyhow!("Failed to expand path: {e}"))
            })?
            .into();

        Ok(config)
    }
}

#[derive(Debug, Parser)]
#[command(name = "puller")]
pub struct Args {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub log_level: Option<String>,
}

/// Get the default configuration file path.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn default_config_path() -> Result<PathBuf> {
    let mut path =
        dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Failed to get config directory"))?;
    path.push("anipler/puller.toml");
    Ok(path)
}

pub struct AniplerPuller {
    artifacts: Mutex<Vec<ArtifactInfo>>,
    auth_header: String,
    base_url: Url,
    destination: PathBuf,
    reqwest: reqwest::Client,
    ssh_host: String,
}

impl AniplerPuller {
    #[must_use]
    pub fn from_config(config: PullerConfig) -> Self {
        Self {
            auth_header: format!("Bearer {}", config.api_key),
            base_url: config.api_url,
            reqwest: reqwest::Client::new(),
            ssh_host: config.ssh_host,
            destination: config.destination,
            artifacts: Mutex::new(Vec::new()),
        }
    }

    /// Fetch the list of artifacts from the server.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn fetch_artifacts_list(&self) -> Result<usize> {
        let url = self
            .base_url
            .join("/api/artifacts")
            .map_err(|e| anyhow::anyhow!("Invalid URL: {e}"))?;

        let response = self
            .reqwest
            .get(url)
            .header("authorization", &self.auth_header)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::OK {
            let mut artifacts = self.artifacts.lock().await;
            let fetched: Vec<ArtifactInfo> = response.json().await?;
            *artifacts = fetched;
            Ok(artifacts.len())
        } else {
            let text = response.text().await?;
            Err(anyhow::anyhow!("Server error: {text}"))
        }
    }

    /// Confirm the transfer of an artifact to the server.
    ///
    /// Return value indicates whether the artifact was newly confirmed `true` or already archived `false`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    async fn confirm(&self, hash: &str) -> Result<bool> {
        let path = format!("/api/artifacts/{hash}/confirm");
        let url = self
            .base_url
            .join(&path)
            .map_err(|e| anyhow::anyhow!("Invalid URL: {e}"))?;

        let response = self
            .reqwest
            .post(url)
            .header("authorization", &self.auth_header)
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::OK => Ok(true),
            reqwest::StatusCode::CONFLICT => Ok(false),
            _ => {
                let text = response.text().await?;
                Err(anyhow::anyhow!("Server error: {text}"))
            }
        }
    }

    /// Transfer the next artifact in the list via rsync.
    ///
    /// Return value indicates whether an artifact was transferred `Some(())` or if there were no artifacts to transfer `None`.
    ///
    /// # Errors
    ///
    /// Returns an error if the transfer or confirmation fails.
    pub async fn transfer_next(&self) -> Result<Option<()>> {
        let mut artifacts = self.artifacts.lock().await;

        let Some(artifact) = &artifacts.last() else {
            return Ok(None);
        };
        let hash = &artifact.hash.clone();

        tracing::info!(hash = %hash, artifact.name, "Transferring artifact");

        let source = format!("{}:{}", self.ssh_host, artifact.path);

        let ssh_cmd = shell_words::join([
            "ssh",
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
        ]);

        let mut rsync_cmd = Command::new("rsync");
        rsync_cmd
            .kill_on_drop(true)
            .args(["--delete", "--partial", "--recursive", "-s"])
            .arg("--rsh")
            .arg(ssh_cmd)
            .arg(source)
            .arg(&self.destination);

        tracing::debug!(command = ?rsync_cmd, "Executing rsync command");

        let output = rsync_cmd
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to execute rsync command: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("rsync failed: {stderr}"));
        }

        artifacts.pop();
        drop(artifacts);

        tracing::info!(hash = %hash, "Artifact transferred successfully, confirming with server");
        self.confirm(hash).await?;

        Ok(Some(()))
    }
}
