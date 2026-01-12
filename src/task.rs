use std::path::Path;

use crate::model::{self, TorrentSource};

pub struct TorrentTaskInfo {
    pub hash: String,
    pub status: TorrentStatus,
    pub content_path: String,
    pub name: String,
}

pub enum TorrentStatus {
    Downloading,
    Seeding,
}

pub struct ArtifactInfo {
    pub hash: String,
}
