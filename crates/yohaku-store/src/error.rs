#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing secret: {0}")]
    MissingSecret(String),
    #[error("{0}")]
    Other(String),
}

pub type StoreResult<T> = Result<T, StoreError>;
