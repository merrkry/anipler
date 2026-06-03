use std::{
    env, fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
};

use ::config::{Config, ConfigBuilder, Environment, File, FileFormat, builder::DefaultState};
use clap::Parser;
use serde::Deserialize;
use url::Url;

const ENV_PREFIX: &str = "ANIPLER";
const DAEMON_CONFIG_PATH_ENV: &str = "ANIPLER_DAEMON_CONFIG_PATH";
const PULLER_CONFIG_PATH_ENV: &str = "ANIPLER_PULLER_CONFIG_PATH";

const DEFAULT_PULL_CRON: &str = "0 0/30 * * * *";
const DEFAULT_TRANSFER_CRON: &str = "0 0 * * * *";
const DEFAULT_API_ADDR: &str = "127.0.0.1:8080";

#[derive(Debug, thiserror::Error)]
pub enum ConfigLoadError {
    #[error("failed to load configuration: {0}")]
    Config(String),
    #[error("failed to determine current directory: {source}")]
    CurrentDirectory { source: io::Error },
    #[error("failed to determine user config directory")]
    ConfigDirectory,
    #[error("failed to expand {field} path {path}: {reason}")]
    PathExpansion {
        field: &'static str,
        path: PathBuf,
        reason: String,
    },
    #[error("{field} path {path} must point to an existing file")]
    PathValidation { field: &'static str, path: PathBuf },
}

impl From<::config::ConfigError> for ConfigLoadError {
    fn from(error: ::config::ConfigError) -> Self {
        Self::Config(error.to_string())
    }
}

#[derive(Debug, Parser)]
#[command(version)]
pub struct DaemonArgs {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long, alias = "no-transfer", default_value_t = false)]
    pub dry_run: bool,
    #[arg(long = "stateless", default_value_t = false)]
    pub stateless: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DaemonConfig {
    pub pull_cron: String,
    pub transfer_cron: String,
    pub qbit: QBitConfig,
    pub storage_path: PathBuf,
    pub stateless: bool,
    pub seedbox: SeedboxConfig,
    pub transfer: TransferConfig,
    pub telegram: TelegramConfig,
    pub api: ApiConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QBitConfig {
    pub url: Url,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeedboxConfig {
    pub ssh_host: String,
    pub ssh_key: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TransferConfig {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub speed_limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub addr: SocketAddr,
    pub key: String,
}

#[derive(Debug, Parser)]
#[command(version)]
pub struct PullerArgs {
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullerConfig {
    pub api_url: Url,
    pub api_key: String,
    pub ssh_host: String,
    pub destination: PathBuf,
}

#[derive(Clone)]
struct ConfigFile {
    path: PathBuf,
    required: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct DaemonConfigOverrides {
    dry_run: Option<bool>,
    stateless: Option<bool>,
}

impl DaemonConfig {
    /// Load daemon configuration from defaults, optional TOML file, environment,
    /// and command-line overrides.
    ///
    /// Configuration file selection is `--config`, then
    /// `ANIPLER_DAEMON_CONFIG_PATH`, then
    /// `$XDG_CONFIG_HOME/anipler/daemon.toml` as an optional file.
    ///
    /// # Errors
    ///
    /// Returns an error if required configuration is missing, if parsing fails,
    /// or if validation fails.
    pub fn load(args: &DaemonArgs) -> Result<Self, ConfigLoadError> {
        let config_file = selected_config_file(
            args.config.as_deref(),
            DAEMON_CONFIG_PATH_ENV,
            "daemon.toml",
        );
        Self::load_from_config_file(
            config_file,
            Some(environment_source(None)),
            DaemonConfigOverrides::from(args),
        )
    }

    #[must_use]
    pub fn transfers_enabled(&self) -> bool {
        !self.transfer.dry_run
    }

    fn load_from_config_file(
        config_file: Option<ConfigFile>,
        env_source: Option<Environment>,
        overrides: DaemonConfigOverrides,
    ) -> Result<Self, ConfigLoadError> {
        let mut builder = daemon_builder()?;

        if let Some(config_file) = config_file {
            builder = builder.add_source(
                File::from(config_file.path)
                    .format(FileFormat::Toml)
                    .required(config_file.required),
            );
        }

        if let Some(env_source) = env_source {
            builder = builder.add_source(env_source);
        }

        let builder = apply_env_alias_overrides(builder, DAEMON_ENV_ALIASES, None)?;
        let builder = apply_daemon_overrides(builder, overrides)?;
        let config = builder.build()?.try_deserialize::<Self>()?;
        config.validate()
    }

    fn validate(mut self) -> Result<Self, ConfigLoadError> {
        self.storage_path = expand_path("storage_path", &self.storage_path)?;
        self.seedbox.ssh_key = expand_path("seedbox.ssh_key", &self.seedbox.ssh_key)?;
        if !validate_file_exists(&self.seedbox.ssh_key) {
            return Err(ConfigLoadError::PathValidation {
                field: "seedbox.ssh_key",
                path: self.seedbox.ssh_key,
            });
        }
        Ok(self)
    }

    #[cfg(test)]
    fn load_from_toml(
        toml: &str,
        env_source: Option<::config::Map<String, String>>,
        overrides: DaemonConfigOverrides,
    ) -> Result<Self, ConfigLoadError> {
        let env_source = env_source.unwrap_or_default();
        let builder = daemon_builder()?
            .add_source(File::from_str(toml, FileFormat::Toml))
            .add_source(environment_source(Some(env_source.clone())));
        let builder = apply_env_alias_overrides(builder, DAEMON_ENV_ALIASES, Some(&env_source))?;
        let builder = apply_daemon_overrides(builder, overrides)?;
        let config = builder.build()?.try_deserialize::<Self>()?;
        config.validate()
    }
}

impl From<&DaemonArgs> for DaemonConfigOverrides {
    fn from(args: &DaemonArgs) -> Self {
        Self {
            dry_run: args.dry_run.then_some(true),
            stateless: args.stateless.then_some(true),
        }
    }
}

impl TransferConfig {
    #[must_use]
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }
}

impl PullerConfig {
    /// Load puller configuration from defaults, optional TOML file, and
    /// environment overrides.
    ///
    /// Configuration file selection is `--config`, then
    /// `ANIPLER_PULLER_CONFIG_PATH`, then
    /// `$XDG_CONFIG_HOME/anipler/puller.toml` as an optional file.
    ///
    /// # Errors
    ///
    /// Returns an error if required configuration is missing, if parsing fails,
    /// or if validation fails.
    pub fn load(args: &PullerArgs) -> Result<Self, ConfigLoadError> {
        let config_file = selected_config_file(
            args.config.as_deref(),
            PULLER_CONFIG_PATH_ENV,
            "puller.toml",
        );
        Self::load_from_config_file(config_file, Some(environment_source(None)))
    }

    /// Load the puller configuration from a specific TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, parsed, or validated.
    pub fn from_path(path: &Path) -> Result<Self, ConfigLoadError> {
        Self::load_from_config_file(
            Some(ConfigFile {
                path: path.to_path_buf(),
                required: true,
            }),
            Some(environment_source(None)),
        )
    }

    fn load_from_config_file(
        config_file: Option<ConfigFile>,
        env_source: Option<Environment>,
    ) -> Result<Self, ConfigLoadError> {
        let mut builder = puller_builder()?;

        if let Some(config_file) = config_file {
            builder = builder.add_source(
                File::from(config_file.path)
                    .format(FileFormat::Toml)
                    .required(config_file.required),
            );
        }

        if let Some(env_source) = env_source {
            builder = builder.add_source(env_source);
        }

        let builder = apply_env_alias_overrides(builder, PULLER_ENV_ALIASES, None)?;
        let config = builder.build()?.try_deserialize::<Self>()?;
        config.validate()
    }

    fn validate(mut self) -> Result<Self, ConfigLoadError> {
        self.destination = expand_path("destination", &self.destination)?;
        Ok(self)
    }

    #[cfg(test)]
    fn load_from_toml(toml: &str) -> Result<Self, ConfigLoadError> {
        let config = puller_builder()?
            .add_source(File::from_str(toml, FileFormat::Toml))
            .build()?
            .try_deserialize::<Self>()?;
        config.validate()
    }
}

const DAEMON_ENV_ALIASES: &[(&str, &str)] = &[
    ("pull_cron", "ANIPLER_PULL_CRON"),
    ("transfer_cron", "ANIPLER_TRANSFER_CRON"),
    ("storage_path", "ANIPLER_STORAGE_PATH"),
    ("seedbox.ssh_host", "ANIPLER_SEEDBOX_SSH_HOST"),
    ("seedbox.ssh_key", "ANIPLER_SEEDBOX_SSH_KEY"),
    ("transfer.dry_run", "ANIPLER_TRANSFER_DRY_RUN"),
    ("telegram.bot_token", "ANIPLER_TELEGRAM_BOT_TOKEN"),
    ("telegram.chat_id", "ANIPLER_TELEGRAM_CHAT_ID"),
];

const PULLER_ENV_ALIASES: &[(&str, &str)] = &[
    ("api_url", "ANIPLER_API_URL"),
    ("api_key", "ANIPLER_API_KEY"),
    ("ssh_host", "ANIPLER_SSH_HOST"),
];

/// Get the default daemon configuration file path.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn default_daemon_config_path() -> Result<PathBuf, ConfigLoadError> {
    default_config_path("daemon.toml")
}

/// Get the default puller configuration file path.
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn default_puller_config_path() -> Result<PathBuf, ConfigLoadError> {
    default_config_path("puller.toml")
}

fn daemon_builder() -> Result<ConfigBuilder<DefaultState>, ConfigLoadError> {
    Ok(Config::builder()
        .set_default("pull_cron", DEFAULT_PULL_CRON)?
        .set_default("transfer_cron", DEFAULT_TRANSFER_CRON)?
        .set_default("stateless", false)?
        .set_default("transfer.dry_run", false)?
        .set_default("api.addr", DEFAULT_API_ADDR)?)
}

fn puller_builder() -> Result<ConfigBuilder<DefaultState>, ConfigLoadError> {
    let destination = env::current_dir()
        .map_err(|source| ConfigLoadError::CurrentDirectory { source })?
        .to_string_lossy()
        .to_string();

    Ok(Config::builder().set_default("destination", destination)?)
}

fn apply_daemon_overrides(
    builder: ConfigBuilder<DefaultState>,
    overrides: DaemonConfigOverrides,
) -> Result<ConfigBuilder<DefaultState>, ConfigLoadError> {
    Ok(builder
        .set_override_option("transfer.dry_run", overrides.dry_run)?
        .set_override_option("stateless", overrides.stateless)?)
}

fn apply_env_alias_overrides(
    mut builder: ConfigBuilder<DefaultState>,
    aliases: &[(&str, &str)],
    source: Option<&::config::Map<String, String>>,
) -> Result<ConfigBuilder<DefaultState>, ConfigLoadError> {
    for (config_key, env_key) in aliases {
        if let Some(value) = env_value(env_key, source) {
            builder = builder.set_override(config_key, value)?;
        }
    }
    Ok(builder)
}

fn env_value(env_key: &str, source: Option<&::config::Map<String, String>>) -> Option<String> {
    match source {
        Some(source) => source
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(env_key))
            .map(|(_, value)| value.clone())
            .filter(|value| !value.is_empty()),
        None => env::var(env_key).ok().filter(|value| !value.is_empty()),
    }
}

fn selected_config_file(
    explicit: Option<&Path>,
    config_path_env: &str,
    default_file_name: &str,
) -> Option<ConfigFile> {
    if let Some(path) = explicit {
        return Some(ConfigFile {
            path: path.to_path_buf(),
            required: true,
        });
    }

    if let Some(path) = env::var_os(config_path_env) {
        return Some(ConfigFile {
            path: PathBuf::from(path),
            required: true,
        });
    }

    default_config_path(default_file_name)
        .ok()
        .map(|path| ConfigFile {
            path,
            required: false,
        })
}

fn default_config_path(file_name: &str) -> Result<PathBuf, ConfigLoadError> {
    let mut path = dirs::config_dir().ok_or(ConfigLoadError::ConfigDirectory)?;
    path.push("anipler");
    path.push(file_name);
    Ok(path)
}

fn environment_source(source: Option<::config::Map<String, String>>) -> Environment {
    Environment::with_prefix(ENV_PREFIX)
        .prefix_separator("_")
        .separator("_")
        .ignore_empty(true)
        .source(source)
}

fn expand_path(field: &'static str, path: &Path) -> Result<PathBuf, ConfigLoadError> {
    let raw_path = path.to_string_lossy().to_string();
    shellexpand::full(&raw_path)
        .map(|path| PathBuf::from(path.into_owned()))
        .map_err(|e| ConfigLoadError::PathExpansion {
            field,
            path: path.to_path_buf(),
            reason: e.to_string(),
        })
}

fn validate_file_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_daemon_toml(ssh_key: &Path) -> String {
        format!(
            r#"
storage_path = "/tmp/anipler"

[qbit]
url = "http://localhost:8081"
username = "admin"
password = "password"

[seedbox]
ssh_host = "seedbox.example"
ssh_key = "{}"

[telegram]
bot_token = "bot-token"
chat_id = 42

[api]
key = "api-key"
"#,
            ssh_key.to_string_lossy()
        )
    }

    fn prefixed_env(entries: &[(&str, &str)]) -> ::config::Map<String, String> {
        let mut source = ::config::Map::new();
        for (key, value) in entries {
            source.insert((*key).to_string(), (*value).to_string());
        }
        source
    }

    #[test]
    fn daemon_config_applies_defaults_and_validation() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ssh_key = temp_dir.path().join("id_ed25519");
        fs::write(&ssh_key, "test-key").unwrap();

        let config = DaemonConfig::load_from_toml(
            &minimal_daemon_toml(&ssh_key),
            None,
            DaemonConfigOverrides::default(),
        )
        .unwrap();

        assert_eq!(config.pull_cron, DEFAULT_PULL_CRON);
        assert_eq!(config.transfer_cron, DEFAULT_TRANSFER_CRON);
        assert_eq!(config.api.addr, DEFAULT_API_ADDR.parse().unwrap());
        assert!(!config.stateless);
        assert!(!config.transfer.is_dry_run());
        assert!(config.transfers_enabled());
        assert_eq!(config.seedbox.ssh_key, ssh_key);
    }

    #[test]
    fn daemon_config_layers_env_and_cli_overrides() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ssh_key = temp_dir.path().join("id_ed25519");
        fs::write(&ssh_key, "test-key").unwrap();

        let env = prefixed_env(&[
            ("ANIPLER_API_ADDR", "127.0.0.1:9000"),
            ("ANIPLER_API_KEY", "00123"),
            ("ANIPLER_TRANSFER_DRY_RUN", "false"),
        ]);
        let overrides = DaemonConfigOverrides {
            dry_run: Some(true),
            stateless: Some(true),
        };

        let config =
            DaemonConfig::load_from_toml(&minimal_daemon_toml(&ssh_key), Some(env), overrides)
                .unwrap();

        assert_eq!(config.api.addr, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.api.key, "00123");
        assert!(config.stateless);
        assert!(config.transfer.is_dry_run());
        assert!(!config.transfers_enabled());
    }

    #[test]
    fn daemon_config_applies_snake_case_env_aliases() {
        let temp_dir = tempfile::tempdir().unwrap();
        let ssh_key = temp_dir.path().join("id_ed25519");
        fs::write(&ssh_key, "test-key").unwrap();

        let env = prefixed_env(&[("ANIPLER_SEEDBOX_SSH_HOST", "env-seedbox.example")]);
        let config = DaemonConfig::load_from_toml(
            &minimal_daemon_toml(&ssh_key),
            Some(env),
            Default::default(),
        )
        .unwrap();

        assert_eq!(config.seedbox.ssh_host, "env-seedbox.example");
    }

    #[test]
    fn config_load_error_does_not_duplicate_config_source() {
        let err = ConfigLoadError::from(::config::ConfigError::NotFound("api.key".into()));

        assert_eq!(
            err.to_string(),
            "failed to load configuration: missing configuration field \"api.key\""
        );
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn daemon_config_rejects_seedbox_key_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let key_dir = temp_dir.path().join("id_ed25519");
        fs::create_dir(&key_dir).unwrap();

        let err = DaemonConfig::load_from_toml(
            &minimal_daemon_toml(&key_dir),
            None,
            DaemonConfigOverrides::default(),
        )
        .unwrap_err();

        assert!(matches!(
            err,
            ConfigLoadError::PathValidation {
                field: "seedbox.ssh_key",
                ..
            }
        ));
    }

    // For example, on NixOS with sops-nix, secrets are accessible at
    // `/run/secrets/foo` which symlinks to `/run/secrets/<generation>/foo`.
    // When using the stable path in config, make sure we preserve the symlink
    // so that change of underlying secrets path doesn't break anything.
    #[cfg(unix)]
    #[test]
    fn daemon_config_preserves_seedbox_key_symlink_path() {
        use std::os::unix::fs as unix_fs;

        let temp_dir = tempfile::tempdir().unwrap();
        let generation_dir = temp_dir.path().join("secrets.d").join("1");
        fs::create_dir_all(&generation_dir).unwrap();
        let target_key = generation_dir.join("id_ed25519");
        fs::write(&target_key, "test-key").unwrap();

        let secrets_dir = temp_dir.path().join("secrets");
        fs::create_dir(&secrets_dir).unwrap();
        let symlink_key = secrets_dir.join("id_ed25519");
        unix_fs::symlink(&target_key, &symlink_key).unwrap();

        let config = DaemonConfig::load_from_toml(
            &minimal_daemon_toml(&symlink_key),
            None,
            DaemonConfigOverrides::default(),
        )
        .unwrap();

        assert_eq!(config.seedbox.ssh_key, symlink_key);
    }

    #[test]
    fn puller_config_loads_from_toml() {
        let temp_dir = tempfile::tempdir().unwrap();
        let destination = temp_dir.path().join("downloads");
        let config = PullerConfig::load_from_toml(&format!(
            r#"
api_url = "http://localhost:8080"
api_key = "api-key"
ssh_host = "relay.example"
destination = "{}"
"#,
            destination.to_string_lossy()
        ))
        .unwrap();

        assert_eq!(config.api_url, Url::parse("http://localhost:8080").unwrap());
        assert_eq!(config.api_key, "api-key");
        assert_eq!(config.ssh_host, "relay.example");
        assert_eq!(config.destination, destination);
    }
}
