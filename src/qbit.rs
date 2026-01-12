use qbit_rs::model::{Credential, GetTorrentListArg, TorrentSource};

use crate::{
    config::DaemonConfig,
    error::AniplerError,
    task::{TorrentStatus, TorrentTaskInfo},
};

const ANIPLER_TORRENT_TAG: &str = "anipler";

pub struct QBitSeedbox {
    endpoint: qbit_rs::Qbit,
}

impl QBitSeedbox {
    pub fn from_config(config: &DaemonConfig) -> Self {
        let credential =
            Credential::new(config.qbit_username.clone(), config.qbit_password.clone());
        let endpoint = qbit_rs::Qbit::new(config.qbit_url.clone(), credential);

        Self { endpoint }
    }

    pub async fn upload_torrent(&self, source: &TorrentSource) -> anyhow::Result<TorrentTaskInfo> {
        unimplemented!()
    }

    /// Query all torrents that should managed by the program from the seedbox.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the response is missing required fields in any torrent.
    pub async fn query_torrents(&self) -> anyhow::Result<Vec<TorrentTaskInfo>> {
        let args = GetTorrentListArg {
            filter: None,
            category: None,
            tag: Some(ANIPLER_TORRENT_TAG.to_string()),
            sort: None,
            reverse: None,
            limit: None,
            offset: None,
            hashes: None,
        };

        self.endpoint
            .get_torrent_list(args)
            .await?
            .into_iter()
            .map(|t| {
                let Some(hash) = t.hash else {
                    return Err(AniplerError::InvalidApiResponse(
                        "Torrent hash missing".into(),
                    ));
                };

                let status = {
                    let Some(progress) = t.progress else {
                        return Err(AniplerError::InvalidApiResponse(
                            "Torrent progress missing".into(),
                        ));
                    };

                    if progress < 1.0 {
                        TorrentStatus::Downloading
                    } else {
                        TorrentStatus::Seeding
                    }
                };

                let Some(content_path) = t.content_path else {
                    return Err(AniplerError::InvalidApiResponse(
                        "Torrent content path missing".into(),
                    ));
                };

                let Some(name) = t.name else {
                    return Err(AniplerError::InvalidApiResponse(
                        "Torrent name missing".into(),
                    ));
                };

                let info = TorrentTaskInfo {
                    hash,
                    status,
                    content_path,
                    name,
                };

                Ok(info)
            })
            .map(|res| res.map_err(anyhow::Error::from))
            .collect()
    }
}
