use crate::config::DaemonConfig;

pub struct AniplerDaemon {
    config: DaemonConfig,
}

impl AniplerDaemon {
    #[must_use]
    pub fn from_config(config: DaemonConfig) -> Self {
        Self { config }
    }
}
