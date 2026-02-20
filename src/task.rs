use std::fmt::Display;

pub use crate::rsync::TransferState;

#[derive(Debug)]
pub struct TorrentTaskInfo {
    pub hash: String,
    pub status: TorrentStatus,
    pub content_path: String,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct TransferTaskInfo {
    /// Unique torrent hash.
    pub hash: String,
    /// rsync source path on seedbox.
    pub source: String,
    /// rsync destination path on relay.
    pub dest: String,
    /// Human-readable torrent name.
    pub name: String,
}

impl Display for TorrentTaskInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}:{:?})", self.hash, self.name)
    }
}

#[derive(Debug)]
pub enum TorrentStatus {
    Downloading,
    Seeding,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct ArtifactInfo {
    pub hash: String,
    pub name: String,
    pub path: String,
}
