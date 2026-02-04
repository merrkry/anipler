use tokio::process::Command;
use tokio::sync::{Semaphore, SemaphorePermit, TryAcquireError};

use crate::config::DaemonConfig;

/// A Rsync-based transmitter for transferring files from remote seedbox.
///
/// To avoid overloading the bandwidth of home network and the complexity of
/// parallel download and resource management, we limit the concurrency
/// of transfer task with semaphore by 1. One must obtain `RsyncTransferSession`
/// to invoke actual transfer.
pub struct RsyncTransmitter {
    executor: RsyncExecutor,
    gate: Semaphore,
}

struct RsyncExecutor {
    ssh_host: String,
    ssh_key_path: String,
    speed_limit: Option<u32>,
    dry_run: bool,
}

pub struct RsyncTransferSession<'a> {
    _permit: SemaphorePermit<'a>,
    executor: &'a RsyncExecutor,
}

impl RsyncTransmitter {
    pub fn from_config(config: &DaemonConfig) -> Self {
        tracing::debug!(host = %config.seedbox_ssh_host, dry_run = config.no_transfer, "Creating rsync transmitter");

        Self {
            executor: RsyncExecutor::from_config(config),
            gate: Semaphore::new(1),
        }
    }

    // Acquire a transfer session, blocking until available.
    #[allow(dead_code)]
    pub async fn session(&self) -> Result<RsyncTransferSession<'_>, RsyncTransmitterError> {
        let permit = self
            .gate
            .acquire()
            .await
            .map_err(|_| RsyncTransmitterError::SemaphoreClosed)?;

        Ok(RsyncTransferSession {
            _permit: permit,
            executor: &self.executor,
        })
    }

    /// Try to acquire a transfer session without blocking.
    pub fn try_session(&self) -> Result<RsyncTransferSession<'_>, RsyncTransmitterError> {
        let permit = self.gate.try_acquire().map_err(|e| match e {
            TryAcquireError::Closed => RsyncTransmitterError::SemaphoreClosed,
            TryAcquireError::NoPermits => RsyncTransmitterError::OverlappingTransfer,
        })?;

        Ok(RsyncTransferSession {
            _permit: permit,
            executor: &self.executor,
        })
    }
}

impl RsyncTransferSession<'_> {
    pub async fn transfer(&self, source: &str, dest: &str) -> Result<(), RsyncTransmitterError> {
        self.executor.transfer(source, dest).await
    }
}

impl RsyncExecutor {
    pub fn from_config(config: &DaemonConfig) -> Self {
        tracing::debug!(host = %config.seedbox_ssh_host, dry_run = config.no_transfer, "Creating rsync transmitter");
        Self {
            ssh_host: config.seedbox_ssh_host.clone(),
            ssh_key_path: config.seedbox_ssh_key.to_string_lossy().to_string(),
            speed_limit: config.rsync_speed_limit,
            dry_run: config.no_transfer,
        }
    }

    pub async fn transfer(&self, source: &str, dest: &str) -> Result<(), RsyncTransmitterError> {
        tracing::info!(source = %source, dest = %dest, "Transferring files");

        let mut rsync_cmd = Command::new("rsync");
        rsync_cmd.args([
            "--delete",
            "--partial",
            "--recursive",
            "-s", // `--protect-args` / `--secluded-args`, use short version for compatibility.
        ]);

        if let Some(limit) = self.speed_limit {
            rsync_cmd.arg("--bwlimit").arg(limit.to_string());
        }

        let ssh_cmd = shell_words::join([
            "ssh",
            "-i",
            &self.ssh_key_path,
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "BatchMode=yes",
        ]);
        rsync_cmd.arg("--rsh").arg(ssh_cmd);

        // Because we use `-s`, no need to escape manually.
        let source_arg = &format!("{}:{}", self.ssh_host, source);
        rsync_cmd.arg(source_arg);
        rsync_cmd.arg(dest);

        tracing::debug!(command = ?rsync_cmd, "Executing rsync command");
        if self.dry_run {
            tracing::info!(source = %source, dest = %dest, "Skipping transfer in dry_run mode");
            return Ok(());
        }

        rsync_cmd
            .output()
            .await
            .map_err(|e| RsyncTransmitterError::RsyncFailed {
                dest: dest.to_string(),
                reason: format!("failed to execute rsync command: {e}"),
            })
            .and_then(|output| {
                if output.status.success() {
                    Ok(output)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Err(RsyncTransmitterError::RsyncFailed {
                        dest: dest.to_string(),
                        reason: format!("rsync exited with code {}: {}", output.status, stderr),
                    })
                }
            })?;

        tracing::info!(dest = %dest, "Artifact available");

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RsyncTransmitterError {
    #[error("semaphore closed")]
    SemaphoreClosed,
    #[error("Rsync command failed for {dest}: {reason}")]
    RsyncFailed { dest: String, reason: String },
    #[error("Another transfer job is already in progress")]
    OverlappingTransfer,
}
