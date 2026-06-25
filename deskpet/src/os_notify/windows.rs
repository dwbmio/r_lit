//! Windows backend: thin wrapper over `winrt-notification` (cross-published
//! as `tauri-winrt-notification`), which builds the toast XML and submits
//! it via the WinRT `ToastNotificationManager`.
//!
//! Without a stable AppUserModelID (AUMID), Windows attributes every toast
//! to the launching shell (often `WshShell.exe`), which is the documented
//! cause of notifications not appearing or grouping into the wrong bucket
//! in Action Center.
//!
//! Override with `DESKPET_AUMID` env var if you ship under multiple
//! install paths or want to share an AUMID with a companion app.
//!
//! [HYPOTHETICAL] The Start Menu shortcut requirement: Windows associates
//! an AUMID with a Start Menu shortcut for proper grouping + icon display.
//! A bare `cargo run` binary without a Start Menu entry will still show
//! toasts, but the app name will fall back to the exe name. The release
//! pipeline installs via `.msi`, which registers a Start Menu shortcut.

use log::info;

use super::{OsNotifier, OsNotifyError};
use crate::notify::Notice;
use tauri_winrt_notification::{Duration as ToastDuration, Toast};

const DEFAULT_AUMID: &str = "Deskpet.Deskpet";

pub struct WindowsNotifier {
    aumid: String,
}

impl WindowsNotifier {
    pub fn new() -> Self {
        let aumid = std::env::var("DESKPET_AUMID")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_AUMID.to_string());
        info!("deskpet: Windows toast under AUMID '{aumid}'");
        Self { aumid }
    }
}

impl Default for WindowsNotifier {
    fn default() -> Self {
        Self::new()
    }
}

impl OsNotifier for WindowsNotifier {
    fn fire(&self, notice: &Notice) -> Result<(), OsNotifyError> {
        let title = notice.title.as_deref().unwrap_or("Deskpet");
        let body = notice.body.clone();
        // Toast::new takes the AppUserModelID. title() is the bold first
        // line; text1() is the body. Duration::Short matches Windows'
        // default non-alarm display.
        //
        // v1: actions are logged but not built into the toast XML. Building
        // proper toast action buttons requires constructing the XML payload
        // with `<actions>` elements and registering a COM activation
        // handler — substantial work that's not worth doing without a
        // concrete click-handling use case.
        if !notice.actions.is_empty() {
            log::info!(
                "deskpet: Windows toast has {} action(s) — labels not yet rendered in toast UI: {:?}",
                notice.actions.len(),
                notice.actions.iter().map(|a| &a.label).collect::<Vec<_>>()
            );
        }
        Toast::new(&self.aumid)
            .title(title)
            .text1(&body)
            .duration(ToastDuration::Short)
            .show()
            .map(|_| ())
            .map_err(|e| OsNotifyError::Fire(format!("winrt-notification: {e}")))
    }

    fn backend_name(&self) -> &'static str {
        "winrt-notification/windows"
    }
}