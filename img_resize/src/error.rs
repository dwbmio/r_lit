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

/// Repo convention: a crate-wide `Result<T>` alias alongside the `thiserror`
/// enum. Some call sites still spell out `Result<_, ReError>` / `Box<dyn Error>`;
/// new code should prefer this alias.
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, ReError>;
