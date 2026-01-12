use core::time;
use std::{env, path::PathBuf};

const ENV_PREFIX: &str = "ANIPLER";

pub struct DaemonConfig {
    pub dry_run: bool,
    pub pull_cron: String,
    pub qbit_url: url::Url,
    pub qbit_username: String,
    pub qbit_password: String,
    pub stateless: bool,
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

        let dry_run = false;

        let pull_cron = "* 0 * * * *".to_string();

        let qbit_url = require_var("QBIT_URL")
            .parse()
            .expect("Environment variable ANIPLER_QBIT_URL must be a valid URL");
        let qbit_username = require_var("QBIT_USERNAME");
        let qbit_password = require_var("QBIT_PASSWORD");

        let stateless = false;

        let storage_path: PathBuf = require_var("STORAGE_PATH").into();

        let transfer_cron = "* * 2 * * *".to_string();

        Self {
            dry_run,
            pull_cron,
            qbit_url,
            qbit_username,
            qbit_password,
            stateless,
            storage_path,
            transfer_cron,
        }
    }
}
