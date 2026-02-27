use thiserror::Error;

/// Result type alias for Murmur operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Error types for Murmur operations.
#[derive(Error, Debug)]
pub enum Error {
    #[cfg(feature = "sqlite-backend")]
    #[error("Storage error: {0}")]
    Storage(#[from] rusqlite::Error),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Election error: {0}")]
    Election(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("{0}")]
    Other(String),
}

// Provide a dummy Storage error variant when SQLite is not enabled
#[cfg(not(feature = "sqlite-backend"))]
impl Error {
    pub(crate) fn _storage_dummy() -> Self {
        Error::Other("Storage error".to_string())
    }
}
