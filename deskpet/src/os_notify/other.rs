//! Noop backend. Compiled in when:
//!
//! - the `os-notify` Cargo feature is off (default dev / test / CI), OR
//! - the target OS is neither macOS nor Windows (Linux, BSDs, wasm, etc.).
//!
//! Lets the rest of the crate code-compile uniformly without any `#[cfg]`
//! at the call site. `make_notifier` returns one of these in any "we don't
//! have a real backend" situation.

use super::{OsNotifier, OsNotifyError};
use crate::notify::Notice;

// Unused on hosts that have a real backend compiled in (the factory selects
// `MacOsNotifier` / `WindowsNotifier` instead). Keep the impl so non-macOS,
// non-Windows targets and `--features os-notify-off` builds still type-check.
#[allow(dead_code)]
pub struct NoopNotifier;

#[allow(dead_code)]
impl NoopNotifier {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl OsNotifier for NoopNotifier {
    fn fire(&self, _notice: &Notice) -> Result<(), OsNotifyError> {
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "noop"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notify::{Level, Notice};
    use std::time::Duration;

    fn sample_notice() -> Notice {
        Notice {
            title: Some("Build".into()),
            body: "3 errors".into(),
            level: Level::Error,
            ttl: Duration::from_millis(9_000),
            id: None,
            expires_at: None,
            actions: vec![],
        }
    }

    #[test]
    fn fire_is_ok() {
        let n = NoopNotifier::new();
        assert!(n.fire(&sample_notice()).is_ok());
    }

    #[test]
    fn fire_with_no_title_is_ok() {
        let mut s = sample_notice();
        s.title = None;
        let n = NoopNotifier::new();
        assert!(n.fire(&s).is_ok());
    }

    #[test]
    fn backend_name_is_stable() {
        assert_eq!(NoopNotifier::new().backend_name(), "noop");
    }

    #[test]
    fn default_matches_new() {
        // Both constructors must yield behaviourally identical notifiers.
        let a = NoopNotifier::default();
        let b = NoopNotifier::new();
        assert_eq!(a.backend_name(), b.backend_name());
        assert!(a.fire(&sample_notice()).is_ok());
    }
}