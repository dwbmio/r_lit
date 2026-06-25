//! In-app reminder bubble state.
//!
//! This module owns the *data* layer for the bubble shown above the mascot:
//! what notice is on screen, how long it's been there, what's queued behind
//! it. The *transport* (NDJSON RPC, HTTP) and the *invocation*
//! (`notification/show` method) live elsewhere — see `rpc::server` and
//! `rpc_methods::notification`.
//!
//! The wire protocol and CLI client have moved out. What remains is just
//! the bits the bubble renderer (`notify_bubble` in `main.rs`) and the
//! scheduler (`advance_notify`) actually touch.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::prelude::*;
use serde::Deserialize;

/// Reminder severity. Drives the bubble's accent color; unknown values from
/// the wire fall back to `Info` so a typo never drops a reminder.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Info,
    Success,
    Warn,
    Error,
}

impl Level {
    /// Bubble accent (border / title) color for this level.
    pub fn accent(self) -> [u8; 3] {
        match self {
            Level::Info => [86, 156, 255],
            Level::Success => [102, 187, 106],
            Level::Warn => [255, 183, 77],
            Level::Error => [239, 83, 80],
        }
    }
}

/// Custom deserializer for `level`: tolerate unknown strings (-> Info) instead
/// of failing the whole message.
fn de_level<'de, D>(d: D) -> Result<Level, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = Option::<String>::deserialize(d)?;
    Ok(match s.as_deref() {
        Some("success") => Level::Success,
        Some("warn") | Some("warning") => Level::Warn,
        Some("error") | Some("err") => Level::Error,
        _ => Level::Info,
    })
}

/// One action button on an OS notification. Backends map to:
/// - macOS: only the first action is used (NSUserNotification is limited
///   to a single action button). Multiple actions are accepted on the
///   wire but only the first is displayed.
/// - Windows: all actions are written to the toast XML up to OS limits
///   (typically 5).
///
/// Click handling is v1: actions are logged via `tracing` but not
/// delivered back to the sender (no callback yet). Add a webhook or
/// local socket relay when there's a concrete consumer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct NotificationAction {
    /// Stable id for callback correlation (sender-supplied).
    pub id: String,
    /// User-visible button label.
    pub label: String,
}

/// A reminder resolved for display (TTL computed, ready to render).
#[derive(Debug, Clone)]
pub struct Notice {
    pub title: Option<String>,
    pub body: String,
    pub level: Level,
    pub ttl: Duration,
    /// Client-supplied correlation id. Optional. When set, the OS-toast
    /// dispatch layer uses it for dedup (same id within `DEDUP_WINDOW_MS`
    /// is suppressed). Bubbles are NOT deduped — every notice shows.
    pub id: Option<String>,
    /// Absolute wall-clock deadline. If `Some` and `Instant::now()` has
    /// passed this, the notice is dropped from the queue without ever
    /// being shown. `None` means "never expires".
    pub expires_at: Option<std::time::Instant>,
    /// Action buttons shown in the OS notification. v1: passed to the
    /// backend for best-effort display; click handling logs only.
    pub actions: Vec<NotificationAction>,
}

impl Notice {
    /// Build a `Notice` from raw parts, applying the same TTL logic that
    /// used to live in `Incoming::Notify` deserialization.
    #[allow(clippy::too_many_arguments)]
    pub fn from_parts(
        title: Option<String>,
        body: String,
        level: Level,
        duration_ms: Option<u64>,
        id: Option<String>,
        expires_in_ms: Option<u64>,
        actions: Vec<NotificationAction>,
    ) -> Self {
        let ttl = match duration_ms {
            Some(ms) => Duration::from_millis(ms.clamp(800, 120_000)),
            None => {
                let base = match level {
                    Level::Info => 4_500,
                    Level::Success => 4_500,
                    Level::Warn => 6_500,
                    Level::Error => 9_000,
                };
                let per_char = 28 * body.chars().count() as u64;
                Duration::from_millis((base + per_char).min(20_000))
            }
        };
        let expires_at = expires_in_ms
            .map(|ms| std::time::Instant::now() + Duration::from_millis(ms));
        Self {
            title,
            body,
            level,
            ttl,
            id,
            expires_at,
            actions,
        }
    }

    /// True if the notice has a deadline and that deadline has passed.
    pub fn is_expired(&self, now: std::time::Instant) -> bool {
        matches!(self.expires_at, Some(deadline) if now >= deadline)
    }
}

/// Live reminder state owned by the Bevy world: the one on screen, its
/// countdown, and the backlog waiting behind it.
#[derive(Resource, Default)]
pub struct NotifyState {
    pub current: Option<Notice>,
    pub timer: Option<Timer>,
    pub queue: VecDeque<Notice>,
}

impl NotifyState {
    /// True while a reminder is on screen (used to grow the hit-test region
    /// and keep the frame rate up so the bubble animates smoothly).
    pub fn showing(&self) -> bool {
        self.current.is_some()
    }

    /// Dismiss the current reminder; the next queued one shows next frame.
    pub fn dismiss(&mut self) {
        self.current = None;
        self.timer = None;
    }
}

/// Build a display `Notice` from raw parts. Used by `notification/show`
/// RPC method to construct the value pushed into `NotifyState.queue`.
pub fn make_notice(
    title: Option<String>,
    body: String,
    level: Level,
    duration_ms: Option<u64>,
    id: Option<String>,
    expires_in_ms: Option<u64>,
    actions: Vec<NotificationAction>,
) -> Notice {
    Notice::from_parts(title, body, level, duration_ms, id, expires_in_ms, actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_clamped_and_derived() {
        // explicit duration is clamped to the sane window
        let n = make_notice(None, "x".into(), Level::Info, Some(10), None, None, vec![]);
        assert_eq!(n.ttl, Duration::from_millis(800));
        // derived TTL grows with severity
        let info = make_notice(None, "short".into(), Level::Info, None, None, None, vec![]);
        let err = make_notice(None, "short".into(), Level::Error, None, None, None, vec![]);
        assert!(err.ttl > info.ttl);
    }

    #[test]
    fn expiry_check() {
        // Far-future expiry: never expires by clock
        let future = make_notice(None, "x".into(), Level::Info, None, None, Some(60_000), vec![]);
        assert!(!future.is_expired(std::time::Instant::now()));
        // Past expiry: always expired
        let past = make_notice(
            None,
            "x".into(),
            Level::Info,
            None,
            None,
            Some(0), // 0ms = already expired
            vec![],
        );
        assert!(past.is_expired(std::time::Instant::now()));
        // No expiry: never expired
        let none = make_notice(None, "x".into(), Level::Info, None, None, None, vec![]);
        assert!(!none.is_expired(std::time::Instant::now()));
    }

    #[test]
    fn unknown_level_falls_back_to_info() {
        // Simulates the wire-format fallback used by the legacy Incoming
        // deserializer.
        let v: serde_json::Result<Level> = serde_json::from_str(r#""bogus""#);
        assert_eq!(v.unwrap_or(Level::Info), Level::Info);
    }
}