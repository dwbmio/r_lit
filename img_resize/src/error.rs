use image::ImageError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ReError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    #[error("{0}")]
    ImageHandleError(#[from] ImageError),

    #[error("{0}")]
    WalkDirError(#[from] walkdir::Error),

    #[error("{0}")]
    ParseError(#[from] yaml_rust::EmitError),

    // #[error("{0}")]
    // TinifyError(#[from] tinify::error::TinifyError),

    #[error("{0}")]
    JsonError(#[from] serde_json::Error),

    #[error("{0}")]
    CustomError(String),
}
