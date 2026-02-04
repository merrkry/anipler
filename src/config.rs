use std::{env, net::SocketAddr, path::PathBuf};

const ENV_PREFIX: &str = "ANIPLER";

#[derive(clap::Parser)]
#[command(version)]
pub struct DaemonArgs {
    #[arg(long = "no-transfer", default_value_t = false)]
    no_transfer: bool,
    #[arg(long = "stateless", default_value_t = false)]
    stateless: bool,
}

pub struct DaemonConfig {
    pub pull_cron: String,
    pub qbit_url: url::Url,
    pub qbit_username: String,
    pub qbit_password: String,
    pub no_transfer: bool,
    pub stateless: bool,
    pub storage_path: PathBuf,
    pub transfer_cron: String,
    pub seedbox_ssh_host: String,
    pub seedbox_ssh_key: PathBuf,
    pub rsync_speed_limit: Option<u32>,
    pub telegram_bot_token: String,
    pub telegram_chat_id: i64,
    pub api_addr: SocketAddr,
    pub api_key: String,
}

impl DaemonConfig {
    /// Load configuration from environment variables.
    ///
    /// # Panics
    ///
    /// Panics if any of the required environment variables is not set.
    #[must_use]
    pub fn from_env(args: &DaemonArgs) -> Self {
        let require_var = |key: &str| {
            let key = format!("{ENV_PREFIX}_{key}");
            env::var(&key).unwrap_or_else(|_| panic!("Environment variable {key} is required"))
        };

        let option_var = |key: &str| {
            let key = format!("{ENV_PREFIX}_{key}");
            env::var(key).ok()
        };

        let pull_cron = "0 0 0/30 * * *".to_string();

        let qbit_url = require_var("QBIT_URL")
            .parse()
            .expect("Environment variable ANIPLER_QBIT_URL must be a valid URL");
        let qbit_username = require_var("QBIT_USERNAME");
        let qbit_password = require_var("QBIT_PASSWORD");

        let no_transfer = args.no_transfer;

        let stateless = args.stateless;

        let storage_path: PathBuf = require_var("STORAGE_PATH").into();

        let transfer_cron = "0 0 * * * *".to_string();

        let seedbox_ssh_host = require_var("SEEDBOX_SSH_HOST");
        let seedbox_ssh_key: PathBuf = std::fs::canonicalize(require_var("SEEDBOX_SSH_KEY"))
            .expect("SEEDBOX_SSH_KEY must point to a valid file");
        let rsync_speed_limit = option_var("RSYNC_SPEED_LIMIT").map(|v| {
            v.parse()
                .expect("ANIPLER_RSYNC_SPEED_LIMIT must be a valid integer")
        });

        let telegram_bot_token = require_var("TELEGRAM_BOT_TOKEN");
        let telegram_chat_id: i64 = require_var("TELEGRAM_CHAT_ID")
            .parse()
            .expect("ANIPLER_TELEGRAM_CHAT_ID must be a valid integer");

        let api_addr: SocketAddr = option_var("API_ADDR")
            .unwrap_or_else(|| "0.0.0.0:8080".to_string())
            .parse()
            .expect("ANIPLER_API_ADDR must be a valid socket address");
        let api_key = require_var("API_KEY");

        Self {
            pull_cron,
            qbit_url,
            qbit_username,
            qbit_password,
            no_transfer,
            stateless,
            storage_path,
            transfer_cron,
            seedbox_ssh_host,
            seedbox_ssh_key,
            rsync_speed_limit,
            telegram_bot_token,
            telegram_chat_id,
            api_addr,
            api_key,
        }
    }
}
