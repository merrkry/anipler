use qbit_rs::model::TorrentSource;

use crate::{config::DaemonConfig, task::TorrentTaskInfo};

const ANIPLER_TORRENT_TAG: &str = "anipler";

pub struct QBitSeedbox {}

impl QBitSeedbox {
    pub const fn from_config(config: &DaemonConfig) -> Self {
        unimplemented!()
    }

    pub async fn upload_torrent(&self, source: &TorrentSource) -> anyhow::Result<TorrentTaskInfo> {
        unimplemented!()
    }

    pub async fn query_torrents(&self) -> anyhow::Result<Vec<TorrentTaskInfo>> {
        unimplemented!()
    }

    pub async fn transfer_torrents(&self) -> anyhow::Result<()> {
        unimplemented!()
    }
}
