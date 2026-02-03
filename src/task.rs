use std::fmt::Display;

#[derive(Debug)]
pub struct TorrentTaskInfo {
    pub hash: String,
    pub status: TorrentStatus,
    pub content_path: String,
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
