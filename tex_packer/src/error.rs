use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("WalkDir error: {0}")]
    WalkDir(#[from] walkdir::Error),

    #[error("No images found in: {0}")]
    NoImages(String),

    #[error("Packing failed: sprites do not fit in {0}x{0} (try increasing --max-size)")]
    PackingFailed(usize),

    #[error("Invalid parameter: {0}")]
    InvalidParam(String),

    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
