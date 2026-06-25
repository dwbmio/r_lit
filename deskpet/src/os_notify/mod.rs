//! OS-level desktop notifications (Notification Center on macOS, Action Center
//! on Windows). Mirrors the in-app bubble so users see reminders even when
//! the mascot is hidden in the tray.
//!
//! # Decoupling contract
//!
//! Three independent boundaries keep the platform surface contained:
//!
//! 1. **Trait boundary.** Callers only see [`OsNotifier`]. No `mac_notification_sys`,
//!    `winrt_notification`, or `objc2` types leak out — the whole point of
//!    this module.
//! 2. **Compilation boundary.** The platform-specific crates are pulled in
//!    via `[target.'cfg(...)'.dependencies]` in `Cargo.toml`. A Linux build
//!    compiles neither; a macOS build compiles only the macOS backend.
//!    This is Cargo's *native* compilation control — no manual `cfg`
//!    feature flag required.
//! 3. **Platform boundary.** macOS code lives in [`macos`], Windows in
//!    [`windows`], everything else (Linux, wasm, etc.) in [`other`].
//!    No `#[cfg]` is interleaved with business logic — each file is
//!    compiled in for exactly one configuration.
//!
//! # Throttle + dedup
//!
//! [`NotificationThrottle`] tracks recently-fired OS toasts so we don't
//! spam Notification Center / Action Center. Two controls:
//!
//! - **Dedup by id** — a notice with `id = "abc"` fired within the
//!   dedup window (default 60s) is suppressed. Bubble is unaffected.
//! - **Rate limit** — min interval between toasts (default 2s) and a
//!   per-minute cap (default 30). Both surfaces skip the OS toast but
//!   keep the bubble visible.
//!
//! All thresholds are env-tunable (`DESKPET_NOTIFY_*`). The bubble path
//! is *not* rate-limited — every notice shows, naturally paced by TTL.
//!
//! # Optional override
//!
//! The Cargo feature `os-notify-off` flips the factory to the noop backend
//! even on supported targets. Use it for CI smoke tests that don't want
//! to actually fire OS notifications:
//!
//! ```text
//! cargo check                                 # real backend on host
//! cargo check --features os-notify-off        # noop backend always
//! ```

mod error;
mod factory;
mod other;
mod throttle;

#[cfg(all(target_os = "macos", not(feature = "os-notify-off")))]
mod macos;
#[cfg(all(target_os = "windows", not(feature = "os-notify-off")))]
mod windows;

use std::sync::Arc;

use bevy::prelude::*;
use log::{info, warn};

use crate::notify::NotifyState;

pub use error::OsNotifyError;
pub use factory::make_notifier;
pub use throttle::{NotificationThrottle, ThrottleConfig, ThrottleDecision};

/// Backend contract. Implementors must be thread-safe (`Send + Sync`) and
/// must be safe to construct at startup (`'static`) since Bevy stores the
/// boxed trait object in a `Resource`.
pub trait OsNotifier: Send + Sync + 'static {
    /// Fire one OS notification. Errors are *non-fatal*: the in-app bubble
    /// is the primary surface and keeps working even if the OS toast fails.
    fn fire(&self, notice: &crate::notify::Notice) -> Result<(), OsNotifyError>;

    /// Stable identifier for logs. Format: `"<crate>/<platform>"` or
    /// `"noop"`. Lets you grep `deskpet-*.log` for `OS notification failed
    /// (mac-notification-sys/macos)` etc. without binding to the impl.
    fn backend_name(&self) -> &'static str;
}

/// Bevy plugin. Register once in `main()`; it installs the notifier resource,
/// the throttle state, and the dispatch system.
pub struct OsNotifyPlugin {
    pub notifier: Arc<dyn OsNotifier>,
}

impl Plugin for OsNotifyPlugin {
    fn build(&self, app: &mut App) {
        // Clone the Arc so we can both log the backend name (immutable borrow)
        // and move a clone into the resource.
        let notifier = Arc::clone(&self.notifier);
        let name = notifier.backend_name();
        let throttle = NotificationThrottle::from_env();
        info!(
            "deskpet: OS notifications enabled (backend: {name}, dedup={}ms, min_interval={}ms, max/min={})",
            throttle.config().dedup_window.as_millis(),
            throttle.config().min_interval.as_millis(),
            throttle.config().max_per_minute,
        );
        app.insert_resource(OsNotify(notifier))
            .insert_resource(throttle)
            .add_systems(Update, dispatch_os_notification);
    }
}

/// Internal: the trait object wrapped in a resource so Bevy can hand it to
/// systems. Not part of the public surface — callers go through the plugin.
#[derive(Resource, Clone)]
struct OsNotify(Arc<dyn OsNotifier>);

/// Edge-detect on `NotifyState.current`: fire one OS toast per new
/// `(title, body, id)` triple. Same notice re-rendering (timer tick, drag,
/// etc.) does not re-fire. Throttle + dedup happens here — not every edge
/// transition results in a toast.
fn dispatch_os_notification(
    state: Res<NotifyState>,
    os: Res<OsNotify>,
    mut throttle: ResMut<NotificationThrottle>,
    mut last: Local<Option<NoticeKey>>,
) {
    let Some(current) = state.current.as_ref() else {
        // Bubble is gone — reset so the next notice fires even if its body
        // happens to match the previous one.
        *last = None;
        return;
    };
    let key = NoticeKey::from(current);
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);

    // Check throttle. The decision is advisory — log it so users can debug
    // "why didn't I get a toast?" without re-reading code.
    let decision = throttle.check_toast(&current.id);
    match decision {
        ThrottleDecision::Fire => {
            throttle.record_toast(current.id.clone());
            if let Err(e) = os.0.fire(current) {
                warn!("deskpet: OS notification failed ({e})");
            }
        }
        ThrottleDecision::SuppressedDuplicate => {
            info!(
                "deskpet: OS toast suppressed (dedup id={:?}) — bubble still shows",
                current.id
            );
        }
        ThrottleDecision::Throttled { reason } => {
            info!(
                "deskpet: OS toast throttled ({reason}) — bubble still shows"
            );
        }
    }
}

/// Identity key for "is this the same notice I already fired for?". Title +
/// body is enough — re-firing with the same words but a different level is
/// unusual and acceptable as a re-fire. (`id` is intentionally excluded so
/// the same body with a new id still fires — id-based dedup happens via
/// the throttle, not the edge key.)
#[derive(PartialEq, Eq, Clone)]
struct NoticeKey {
    title: Option<String>,
    body: String,
}

impl From<&crate::notify::Notice> for NoticeKey {
    fn from(n: &crate::notify::Notice) -> Self {
        Self {
            title: n.title.clone(),
            body: n.body.clone(),
        }
    }
}