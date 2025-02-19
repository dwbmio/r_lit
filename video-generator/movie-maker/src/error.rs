use std::io;

use ffmpeg_next;
use image::ImageError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MovieError {
    #[error("{0}")]
    FFmpegError(#[from] ffmpeg_next::util::error::Error),

    #[error("{0}")]
    CustomError(String),

    #[error("{0}")]
    ImageLocLoadError(#[from] ImageError),

    // parse config 
    #[error("{0}")]
    IoConfigError(#[from] io::Error),
    #[error("{0}")]
    ParseConfigError(#[from] serde_json::error::Error)
}

