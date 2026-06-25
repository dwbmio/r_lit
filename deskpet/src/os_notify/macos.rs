//! macOS backend: thin wrapper over `mac-notification-sys`, which speaks
//! `NSUserNotificationCenter` directly via objc2.
//!
//! [HYPOTHETICAL] On macOS 11+ Apple has deprecated `NSUserNotification` in
//! favour of the UserNotifications framework (`UNUserNotificationCenter`).
//! `mac-notification-sys` still uses the deprecated path, which works on
//! recent macOS but may eventually stop showing toasts for unbundled CLI
//! binaries. Migrating to `mac-usernotifications` (UserNotifications) is a
//! future-only task — it's not load-bearing for the current feature.

use log::{info, warn};

use super::{OsNotifier, OsNotifyError};
use crate::notify::Notice;

/// Default bundle identifier used when the running binary's Info.plist does
/// not carry one (bare `cargo run` workflow). The release `.app` bundle
/// overrides this via the bundle's own Info.plist.
const DEFAULT_BUNDLE_ID: &str = "com.deskpet.app";

pub struct MacOsNotifier {
    // Kept for future use (e.g., logging on fire failure, or surfacing in
    // a settings panel). Currently set in `new()` and read once at startup.
    #[allow(dead_code)]
    bundle_id: String,
    #[allow(dead_code)]
    initialized: bool,
}

impl MacOsNotifier {
    pub fn new() -> Self {
        let bundle_id = std::env::var("DESKPET_BUNDLE_ID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_BUNDLE_ID.to_string());

        // Tell NSUserNotificationCenter which app the notification comes from.
        // mac-notification-sys resolves the real CFBundleIdentifier from the
        // running .app's Info.plist at runtime; we pass this only as a
        // fallback for unbundled execution.
        let initialized = mac_notification_sys::set_application(&bundle_id).is_ok();
        if initialized {
            info!("deskpet: macOS notifications via NSUserNotificationCenter (bundle id '{bundle_id}')");
        } else {
            warn!(
                "deskpet: macOS notification permission / bundle id setup failed \
                 (bundle id '{bundle_id}') — toasts may not appear"
            );
        }

        Self {
            bundle_id,
            initialized,
        }
    }
}

impl Default for MacOsNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl OsNotifier for MacOsNotifier {
    fn fire(&self, notice: &Notice) -> Result<(), OsNotifyError> {
        let title = notice.title.as_deref().unwrap_or("Deskpet");
        let body = &notice.body;
        // v1: log action labels but don't render buttons — mac-notification-sys
        // 0.6 (NSUserNotification) doesn't expose a builder API for the action
        // button title. Switching to `mac-usernotifications` (UserNotifications
        // framework) would give proper buttons but is a bigger refactor for
        // unclear ROI without a click-handling consumer.
        if !notice.actions.is_empty() {
            log::info!(
                "deskpet: macOS toast actions (logged only, not rendered): {:?}",
                notice.actions.iter().map(|a| (&a.id, &a.label)).collect::<Vec<_>>()
            );
        }
        mac_notification_sys::send_notification(title, None, body, None)
            .map(|_| ())
            .map_err(|e| OsNotifyError::Fire(format!("mac-notification-sys: {e}")))
    }

    fn backend_name(&self) -> &'static str {
        "mac-notification-sys/macos"
    }
}