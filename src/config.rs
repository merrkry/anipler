use std::env;

const ENV_PREFIX: &str = "ANIPLER";

pub struct DaemonConfig {
    pub storage_path: String,
    pub db_path: Option<String>,
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

        let storage_path = require_var("STORAGE_PATH");

        let db_path = std::env::var(format!("{ENV_PREFIX}_DB_PATH")).ok();

        Self {
            storage_path,
            db_path,
        }
    }
}
