use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("HTTP request error: {0}")]
    ReqwestError(#[from] reqwest::Error),

    #[error("S3 config error: {0}")]
    S3ConfigError(String),

    #[error("S3 put object error: {0}")]
    S3PutError(String),

    #[error("Download failed for url={0}: {1}")]
    DownloadFailed(String, String),

    #[error("{0}")]
    CustomError(String),
}

// aws S3 SDK 的错误类型是泛型的，无法直接 #[from]，用手动 impl 转换
impl<E: std::fmt::Display> From<aws_sdk_s3::error::SdkError<E>> for AppError {
    fn from(e: aws_sdk_s3::error::SdkError<E>) -> Self {
        AppError::S3PutError(e.to_string())
    }
}
