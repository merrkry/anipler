use tokio::process::Command;

use crate::config::DaemonConfig;
use crate::error::AniplerError;

pub struct RsyncTransmitter {
    ssh_host: String,
    ssh_key_path: String,
    speed_limit: Option<u32>,
    dry_run: bool,
}

impl RsyncTransmitter {
    pub fn from_config(config: &DaemonConfig) -> Self {
        Self {
            ssh_host: config.seedbox_ssh_host.clone(),
            ssh_key_path: config.seedbox_ssh_key.to_string_lossy().to_string(),
            speed_limit: config.rsync_speed_limit,
            dry_run: config.dry_run,
        }
    }

    pub async fn transfer(&self, source: &str, dest: &str) -> Result<(), AniplerError> {
        log::info!("Transferring {source} -> {dest}");

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

        log::debug!("Would execute command: {rsync_cmd:?}");
        if self.dry_run {
            return Ok(());
        }

        rsync_cmd
            .output()
            .await
            .map_err(|e| AniplerError::RsyncFailed {
                dest: dest.to_string(),
                reason: format!("Failed to execute rsync: {e}"),
            })
            .and_then(|output| {
                if output.status.success() {
                    Ok(output)
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Err(AniplerError::RsyncFailed {
                        dest: dest.to_string(),
                        reason: stderr,
                    })
                }
            })?;

        log::info!("Artifact available at: {dest}");

        Ok(())
    }
}
