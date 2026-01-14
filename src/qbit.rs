use chrono::{DateTime, Utc};
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

    pub async fn upload_torrent(&self, _source: &TorrentSource) -> anyhow::Result<TorrentTaskInfo> {
        unimplemented!()
    }

    /// Query all torrents that should managed by the program from the seedbox.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails or the response is missing required fields in any torrent.
    pub async fn query_torrents(
        &self,
        earliest_import_date: DateTime<Utc>,
    ) -> anyhow::Result<Vec<TorrentTaskInfo>> {
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

        let mut ignored_count = 0;
        let mut tracked_count = 0;

        let torrents = self
            .endpoint
            .get_torrent_list(args)
            .await?
            .into_iter()
            .filter_map(|t| {
                macro_rules! extract_filed {
                    ($opt:expr, $field:expr) => {{
                        let Some(value) = $opt else {
                            return Some(Err(AniplerError::InvalidApiResponse(format!(
                                "Missing field {} in torrent info",
                                $field
                            ))));
                        };
                        value
                    }};
                }

                let hash = extract_filed!(t.hash, "hash");
                let status = {
                    let progress = extract_filed!(t.progress, "progress");

                    if progress < 1.0 {
                        TorrentStatus::Downloading
                    } else {
                        TorrentStatus::Seeding
                    }
                };
                let content_path = extract_filed!(t.content_path, "content_path");
                let name = extract_filed!(t.name, "name");

                let info = TorrentTaskInfo {
                    hash,
                    status,
                    content_path,
                    name,
                };

                let added_on = extract_filed!(t.added_on, "added_on");
                if added_on < earliest_import_date.timestamp() {
                    ignored_count += 1;
                    log::trace!("Ignoring torrent: {info}");
                    return None;
                }

                tracked_count += 1;
                log::trace!("Tracking torrent: {info}");

                Some(Ok(info))
            })
            .map(|res| res.map_err(anyhow::Error::from))
            .collect::<anyhow::Result<_>>()?;

        log::debug!(
            "Queried torrents from API: tracked {tracked_count}, ignored {ignored_count}"
        );

        Ok(torrents)
    }
}
