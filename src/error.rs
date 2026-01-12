#[derive(Debug, thiserror::Error)]
pub enum AniplerError {
    #[error("API responded with an invalid response: {0}")]
    InvalidApiResponse(String),
}
