use thiserror::Error;

/// Crate-wide error enum, following the repo convention of a `thiserror`
/// enum + `Result<T>` alias per crate. Reserved for the headless/setup paths;
/// the live mascot loop tolerates missing data and retries next frame.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum DeskpetError {
    /// No monitor was reported by the windowing backend, so we cannot
    /// compute the desktop bounds the mascot walks within.
    #[error("no monitor detected by the windowing backend")]
    NoMonitor,
}

#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, DeskpetError>;
