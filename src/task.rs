use crate::model::{self, TorrentSource};

#[derive(derive_builder::Builder)]
#[builder(setter(into))]
pub struct TorrentTaskInfo {
    pub hash: String,
    pub status: TorrentStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TorrentStatus {
    Downloading,
    Seeding,
}

#[derive(derive_builder::Builder)]
#[builder(setter(into))]
pub struct ArtifactInfo {
    pub hash: String,
}
