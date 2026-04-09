use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Font not found: {0}")]
    FontNotFound(String),
    #[error("Font loading error: {0}")]
    FontLoad(String),
    #[error("Invalid color: {0}")]
    ColorParse(String),
    #[error("Unknown effect: {0}")]
    UnknownEffect(String),
    #[error("Invalid effect parameter: {0}")]
    InvalidEffectParam(String),
    #[error("Render error: {0}")]
    Render(String),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("PNG encoding error: {0}")]
    PngEncode(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
