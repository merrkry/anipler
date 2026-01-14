#[derive(Debug, thiserror::Error)]
pub enum AniplerError {
    #[error("API responded with an invalid response: {0}")]
    InvalidApiResponse(String),
    #[error("Rsync command failed for {dest}: {reason}")]
    RsyncFailed { dest: String, reason: String },
    #[error("Another transfer job is already in progress")]
    OverlappingTransferJob,
}
