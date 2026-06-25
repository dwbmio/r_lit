use thiserror::Error;

/// Failure modes for OS notification dispatch. All variants are *non-fatal*:
/// the in-app bubble is the primary surface and continues to work even when
/// the OS toast fails.
///
/// Note: variants may be unused in some build configurations (e.g. noop
/// backend never returns `Err`). They are part of the public trait contract
/// and stay in the enum regardless.
#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum OsNotifyError {
    /// The backend (notify-rust, the OS framework beneath it, etc.) refused
    /// to display the toast. Common causes: permission denied (macOS
    /// without bundle ID), missing AppUserModelID shortcut (Windows),
    /// notification center disabled by user.
    #[error("OS notification backend error: {0}")]
    Fire(String),

    /// Backend was requested at runtime but is not compiled into this build.
    /// This should never surface to a user — `make_notifier` returns the
    /// noop backend in this case — kept for API completeness.
    #[error("OS notification feature is disabled in this build")]
    Disabled,
}