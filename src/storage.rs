use std::path::PathBuf;

use sqlx::Connection;
use tokio::sync::RwLock;

use crate::{
    config::DaemonConfig,
    task::{ArtifactInfo, TorrentTaskInfo},
};

pub struct StorageManager {
    state: RwLock<StorageState>,
}

struct StorageState {
    storage_path: PathBuf,
    db: sqlx::SqliteConnection,
}

impl StorageManager {
    pub async fn from_config(config: &DaemonConfig) -> anyhow::Result<Self> {
        let storage_path = config.storage_path.clone();

        let db_path = storage_path.join("storage.db");
        let db_url = format!("sqlite://{}", db_path.to_string_lossy().to_string());

        let db = sqlx::SqliteConnection::connect(&db_url).await?;

        let state = RwLock::new(StorageState { storage_path, db });

        Ok(Self { state })
    }

    /// Update information about the given torrents.
    pub async fn update_torrent_info(&self, torrents: &[TorrentTaskInfo]) -> anyhow::Result<()> {
        unimplemented!();
    }

    /// List all torrents that are ready, i.e. have finished downloading and are ready for
    /// transmission.
    pub async fn list_ready_torrents(&self) -> anyhow::Result<Vec<TorrentTaskInfo>> {
        unimplemented!();
    }

    /// Prepare storage for an artifact with the given hash.
    pub async fn prepare_artifact_storage(&self, hash: &str) -> anyhow::Result<ArtifactInfo> {
        unimplemented!();
    }

    /// Mark an artifact as ready for archival.
    pub async fn mark_artifact_ready(&self, hash: &str) -> anyhow::Result<()> {
        unimplemented!();
    }

    /// List all artifacts that are ready for archival.
    pub async fn list_ready_artifacts(&self) -> anyhow::Result<Vec<ArtifactInfo>> {
        unimplemented!();
    }

    /// Reclaim storage used by the artifact with the given hash.
    pub async fn reclaim_artifact_storage(&self, hash: &str) -> anyhow::Result<()> {
        unimplemented!();
    }
}
