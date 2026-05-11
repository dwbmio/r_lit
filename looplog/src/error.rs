use std::io;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("time parse error: {0}")]
    TimeParse(String),
    #[error("invalid metadata entry `{0}`, expected key=value")]
    InvalidMeta(String),
    #[error("missing command after `--`")]
    MissingCommand,
    #[error("http error: {0}")]
    Http(String),
    #[error("{0}")]
    Message(String),
}

impl From<Box<dyn std::error::Error + Send + Sync>> for Error {
    fn from(value: Box<dyn std::error::Error + Send + Sync>) -> Self {
        Self::Http(value.to_string())
    }
}
