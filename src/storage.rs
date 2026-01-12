use tokio::sync::RwLock;

use crate::config::DaemonConfig;

pub struct StorageManager {
    storage_path: String,
    db: sqlx::SqliteConnection,
}

impl StorageManager {
    pub fn from_config(config: &DaemonConfig) -> Self {
        Self {
            storage_path: config.storage_path.clone(),
            db: unimplemented!(),
        }
    }
}
