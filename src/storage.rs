use std::path::PathBuf;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

use crate::{
    config::DaemonConfig,
    task::{ArtifactInfo, TorrentStatus, TorrentTaskInfo},
};

const EARLIEST_IMPORT_DATE_KEY: &str = "earliest_import_date";

#[derive(Debug, thiserror::Error)]
pub enum FinalizeArtifactError {
    #[error("Artifact not found")]
    NotFound,
    #[error("Artifact already archived")]
    AlreadyArchived,
    #[error("Storage error: {0}")]
    Storage(#[from] anyhow::Error),
}

/// Status of a managed torrenting task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u64)]
pub enum TaskStatus {
    /// Torrent is being tracked (downloading).
    Tracked = 0,
    /// Torrent is seeding, ready for transfer to relay.
    TorrentReady = 1,
    /// Torrent is on relay, ready for pull.
    ArtifactReady = 2,
    /// Pull complete, preserve the record so that it won't be re-tracked.
    Archived = 3,
}

pub struct StorageManager {
    state: RwLock<StorageState>,
    storage_path: PathBuf,
}

struct StorageState {
    db: sqlx::sqlite::SqlitePool,
}

impl StorageManager {
    pub async fn from_config(config: &DaemonConfig) -> Result<Self, StorageManagerError> {
        let storage_path = config.storage_path.clone();

        let db_path = storage_path.join("storage.db");
        let db_url = if config.stateless {
            "sqlite::memory:".to_string()
        } else {
            format!("sqlite://{}?mode=rwc", db_path.to_string_lossy())
        };

        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .connect(&db_url)
            .await?;

        let state = RwLock::new(StorageState { db });

        let this = Self {
            state,
            storage_path,
        };

        this.init().await?;

        Ok(this)
    }

    /// Initialize the storage backend.
    ///
    /// # Errors
    ///
    /// Returns an error if database initialization fails.
    pub async fn init(&self) -> Result<(), StorageManagerError> {
        sqlx::query(
            r"
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tasks (
  hash TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  status INTEGER NOT NULL,
  content_path TEXT NOT NULL
);
            ",
        )
        .execute(&self.state.write().await.db)
        .await?;

        Ok(())
    }

    /// Get the earliest import date for torrents to be managed.
    /// Current time will be recorded if not exists.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub async fn earliest_import_date(&self) -> Result<DateTime<Utc>, StorageManagerError> {
        let state = self.state.write().await;

        let record: Option<(String,)> =
            sqlx::query_as(r"SELECT value FROM settings WHERE key = $1")
                .bind(EARLIEST_IMPORT_DATE_KEY)
                .fetch_optional(&state.db)
                .await?;

        if let Some((value,)) = record {
            let datetime = DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc);
            return Ok(datetime);
        }

        let now = Utc::now();
        sqlx::query(
            r"
INSERT INTO settings (key, value)
VALUES ($1, $2)
ON CONFLICT(key) DO UPDATE SET value = EXCLUDED.value;
            ",
        )
        .bind(EARLIEST_IMPORT_DATE_KEY)
        .bind(now.to_rfc3339())
        .execute(&state.db)
        .await?;

        drop(state);

        Ok(now)
    }

    /// Update information about the given torrents.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub async fn update_torrent_info(&self, torrents: &[TorrentTaskInfo]) -> Result<(), StorageManagerError> {
        let state = self.state.read().await;

        for t in torrents {
            let status = match t.status {
                TorrentStatus::Downloading => TaskStatus::Tracked,
                TorrentStatus::Seeding => TaskStatus::TorrentReady,
            };

            sqlx::query(
                r"
INSERT INTO tasks (hash, name, status, content_path)
VALUES ($1, $2, $3, $4)
ON CONFLICT(hash) DO UPDATE SET
  name = EXCLUDED.name,
  status = EXCLUDED.status,
  content_path = EXCLUDED.content_path
WHERE tasks.status < EXCLUDED.status
                ",
            )
            .bind(&t.hash)
            .bind(&t.name)
            .bind(status as i64)
            .bind(&t.content_path)
            .execute(&state.db)
            .await?;
        }

        drop(state);

        Ok(())
    }

    /// List all torrents that are ready for pulling.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub async fn list_ready_torrents(&self) -> Result<Vec<TorrentTaskInfo>, StorageManagerError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            hash: String,
            name: String,
            content_path: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r"
SELECT hash, name, content_path
FROM tasks
WHERE status = $1
            ",
        )
        .bind(TaskStatus::TorrentReady as i64)
        .fetch_all(&self.state.read().await.db)
        .await?;

        let torrents = rows
            .into_iter()
            .map(|row| TorrentTaskInfo {
                hash: row.hash,
                name: row.name,
                status: TorrentStatus::Seeding,
                content_path: row.content_path,
            })
            .collect();

        Ok(torrents)
    }

    /// Mark a torrent as ready for pulling.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub async fn mark_artifact_ready(&self, hash: &str) -> Result<(), StorageManagerError> {
        sqlx::query(
            r"
UPDATE tasks
SET status = $1
WHERE hash = $2 AND status = $3
            ",
        )
        .bind(TaskStatus::ArtifactReady as i64)
        .bind(hash)
        .bind(TaskStatus::TorrentReady as i64)
        .execute(&self.state.write().await.db)
        .await?;

        Ok(())
    }

    /// List all artifacts that are ready for archiving.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail.
    pub async fn list_ready_artifacts(&self) -> Result<Vec<ArtifactInfo>, StorageManagerError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            hash: String,
            name: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r"
SELECT hash, name
FROM tasks
WHERE status = $1
            ",
        )
        .bind(TaskStatus::ArtifactReady as i64)
        .fetch_all(&self.state.read().await.db)
        .await?;

        let artifacts = rows
            .into_iter()
            .map(|row| {
                let path = self
                    .artifact_storage_path(&row.hash)
                    .to_string_lossy()
                    .into();

                ArtifactInfo {
                    hash: row.hash,
                    name: row.name,
                    path,
                }
            })
            .collect();

        Ok(artifacts)
    }

    /// Get the path of artifact on relay from the given hash.
    pub fn artifact_storage_path(&self, hash: &str) -> PathBuf {
        self.storage_path.join("artifacts").join(hash)
    }

    /// Prepare the artifact storage directory.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails.
    pub async fn prepare_artifact_storage(&self, hash: &str) -> Result<(), StorageManagerError> {
        tokio::fs::create_dir_all(self.artifact_storage_path(hash)).await?;
        Ok(())
    }

    /// Mark a torrent as archived and deletes its folder.
    ///
    /// # Errors
    ///
    /// Returns an error if database queries fail, or directory deletion fails.
    pub async fn finalize_artifact(&self, hash: &str) -> Result<(), FinalizeArtifactError> {
        tracing::info!(hash = %hash, "Finalizing artifact");

        let result = sqlx::query(
            r"
UPDATE tasks
SET status = $1
WHERE hash = $2 AND status = $3
            ",
        )
        .bind(TaskStatus::Archived as i64)
        .bind(hash)
        .bind(TaskStatus::ArtifactReady as i64)
        .execute(&self.state.write().await.db)
        .await
        .map_err(|e| FinalizeArtifactError::Storage(anyhow::anyhow!(e)))?;

        if result.rows_affected() == 0 {
            let already_archived = sqlx::query(
                r"
SELECT 1 FROM tasks WHERE hash = $1 AND status = $2
                ",
            )
            .bind(hash)
            .bind(TaskStatus::Archived as i64)
            .fetch_optional(&self.state.write().await.db)
            .await
            .map_err(|e| FinalizeArtifactError::Storage(anyhow::anyhow!(e)))?
            .is_some();

            return if already_archived {
                Err(FinalizeArtifactError::AlreadyArchived)
            } else {
                Err(FinalizeArtifactError::NotFound)
            };
        }

        let path = self.artifact_storage_path(hash);
        tracing::info!(path = %path.display(), "Removing artifact storage directory");

        tokio::fs::remove_dir_all(path)
            .await
            .map_err(|e| FinalizeArtifactError::Storage(anyhow::anyhow!(e)))?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageManagerError {
    #[error("I/O error: {0}")]
    Io(#[from] tokio::io::Error),
    #[error("Database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("Parsing error: {0}")]
    Chrono(#[from] chrono::ParseError),
}
