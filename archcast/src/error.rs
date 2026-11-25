use rlit_cli::ErrorKind;
use std::path::PathBuf;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ArchcastError>;

#[derive(Debug, Error)]
pub enum ArchcastError {
    #[error("invalid usage: {0}")]
    Usage(String),

    #[error("input file not found or unreadable ({path}): {source}")]
    ReadInput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write output ({path}): {source}")]
    WriteOutput {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse YAML spec ({path}): {source}")]
    YamlParse {
        path: PathBuf,
        #[source]
        source: serde_yaml_ng::Error,
    },

    #[error("failed to parse JSON spec ({path}): {source}")]
    JsonParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to serialize YAML: {0}")]
    YamlSerialize(#[from] serde_yaml_ng::Error),

    #[error("failed to parse SVG/XML in {path}: {message}")]
    XmlParse { path: PathBuf, message: String },

    #[error("spec validation failed with {count} issue(s)")]
    ValidationFailed { count: usize },

    #[error("no matching input files found in {0}")]
    NoInputs(PathBuf),

    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),

    #[error("rasterization / animation error: {0}")]
    Raster(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl ArchcastError {
    pub fn kind(&self) -> ErrorKind {
        match self {
            ArchcastError::Usage(_) | ArchcastError::BackendUnavailable(_) => ErrorKind::Usage,
            ArchcastError::ReadInput { .. }
            | ArchcastError::YamlParse { .. }
            | ArchcastError::JsonParse { .. }
            | ArchcastError::XmlParse { .. }
            | ArchcastError::ValidationFailed { .. }
            | ArchcastError::NoInputs(_) => ErrorKind::Input,
            ArchcastError::WriteOutput { source, .. } | ArchcastError::Io(source) => {
                rlit_cli::io_error_kind(source)
            }
            ArchcastError::YamlSerialize(_) | ArchcastError::Raster(_) => ErrorKind::Runtime,
        }
    }
}
