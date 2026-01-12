use core::time;
use std::{env, path::PathBuf};

const ENV_PREFIX: &str = "ANIPLER";

pub struct DaemonConfig {
    pub db_path: Option<PathBuf>,
    pub dry_run: bool,
    pub pull_cron: String,
    pub storage_path: PathBuf,
    pub transfer_cron: String,
}

impl DaemonConfig {
    /// Load configuration from environment variables.
    ///
    /// # Panics
    ///
    /// Panics if any of the required environment variables is not set.
    #[must_use]
    pub fn from_env() -> Self {
        let require_var = |key: &str| {
            let key = format!("{ENV_PREFIX}_{key}");
            env::var(&key).unwrap_or_else(|_| panic!("Environment variable {key} is required"))
        };

        let storage_path: PathBuf = require_var("STORAGE_PATH").into();

        let db_path = Some(storage_path.join("anipler.db"));

        let pull_cron = "* 0 * * * *".to_string();

        let dry_run = false;

        let transfer_cron = "* * 2 * * *".to_string();

        Self {
            db_path,
            dry_run,
            pull_cron,
            storage_path,
            transfer_cron,
        }
    }
}
