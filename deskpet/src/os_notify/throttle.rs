//! OS-toast throttle + dedup state.
//!
//! Two controls:
//! 1. **Dedup by id** — a notice with the same `id` within the dedup window
//!    is suppressed (no toast fires; bubble still shows).
//! 2. **Rate limit** — minimum interval between toasts, plus a per-minute
//!    rolling cap. Both throttle types result in no toast fire; bubble
//!    is unaffected.
//!
//! Both controls apply to the *OS toast* surface only. The bubble is
//! rate-limited by its own TTL (5-9s per notice) so the user sees
//! everything; OS Notification Center doesn't get spammed.
//!
//! All thresholds are env-tunable. Defaults:
//! - `DESKPET_NOTIFY_DEDUP_WINDOW_MS` = 60_000  (60s)
//! - `DESKPET_NOTIFY_MIN_INTERVAL_MS` = 2_000   (2s)
//! - `DESKPET_NOTIFY_MAX_PER_MINUTE`  = 30      (30/min)

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use bevy::prelude::Resource;

/// Tunable thresholds. Constructed once at startup from env vars.
#[derive(Clone, Debug)]
pub struct ThrottleConfig {
    /// Window during which a repeated `id` is suppressed. Default 60s.
    pub dedup_window: Duration,
    /// Minimum time between two OS toasts. Default 2s.
    pub min_interval: Duration,
    /// Maximum OS toasts in any rolling 60s window. Default 30.
    pub max_per_minute: u32,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            dedup_window: Duration::from_secs(60),
            min_interval: Duration::from_millis(2_000),
            max_per_minute: 30,
        }
    }
}

impl ThrottleConfig {
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(s) = std::env::var("DESKPET_NOTIFY_DEDUP_WINDOW_MS") {
            if let Ok(ms) = s.parse::<u64>() {
                cfg.dedup_window = Duration::from_millis(ms);
            }
        }
        if let Ok(s) = std::env::var("DESKPET_NOTIFY_MIN_INTERVAL_MS") {
            if let Ok(ms) = s.parse::<u64>() {
                cfg.min_interval = Duration::from_millis(ms);
            }
        }
        if let Ok(s) = std::env::var("DESKPET_NOTIFY_MAX_PER_MINUTE") {
            if let Ok(n) = s.parse::<u32>() {
                cfg.max_per_minute = n;
            }
        }
        cfg
    }
}

/// Result of a throttle check. Tells the dispatcher whether to fire and,
/// if not, why not (for logging).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleDecision {
    /// All clear — fire the OS toast.
    Fire,
    /// Same `id` was seen within the dedup window.
    SuppressedDuplicate,
    /// Either too soon since last toast, or per-minute cap reached.
    Throttled { reason: ThrottleReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleReason {
    MinInterval,
    PerMinuteCap,
}

impl std::fmt::Display for ThrottleReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MinInterval => "min_interval",
            Self::PerMinuteCap => "per_minute_cap",
        })
    }
}

impl ThrottleDecision {
    pub fn is_fire(self) -> bool {
        matches!(self, ThrottleDecision::Fire)
    }
}

/// Live throttle state. Lives in a Bevy `Resource` so multiple systems can
/// observe + update it (notification/show method peeks for `suppressed`
/// hint, dispatch_os_notification checks + records).
#[derive(Resource, Clone)]
pub struct NotificationThrottle {
    config: ThrottleConfig,
    /// Recently-seen ids → when they were last seen. Bounded by `seen_cap`.
    seen_ids: HashMap<String, Instant>,
    /// When the last OS toast was fired (for min-interval check).
    last_toast: Option<Instant>,
    /// Timestamps of the last N toasts (for per-minute cap check).
    recent_toasts: VecDeque<Instant>,
}

impl NotificationThrottle {
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config,
            seen_ids: HashMap::new(),
            last_toast: None,
            recent_toasts: VecDeque::new(),
        }
    }

    pub fn from_env() -> Self {
        Self::new(ThrottleConfig::from_env())
    }

    pub fn config(&self) -> &ThrottleConfig {
        &self.config
    }

    /// Pure check: should the OS toast fire? Does NOT mutate state.
    /// Use this when you only want to *peek* the decision (e.g., to set
    /// `suppressed=true` on an RPC response without actually firing).
    pub fn check_toast(&self, id: &Option<String>) -> ThrottleDecision {
        let now = Instant::now();
        if let Some(id) = id.as_ref() {
            if let Some(seen_at) = self.seen_ids.get(id) {
                if now.duration_since(*seen_at) < self.config.dedup_window {
                    return ThrottleDecision::SuppressedDuplicate;
                }
            }
        }
        if let Some(last) = self.last_toast {
            if now.duration_since(last) < self.config.min_interval {
                return ThrottleDecision::Throttled {
                    reason: ThrottleReason::MinInterval,
                };
            }
        }
        let in_window = self
            .recent_toasts
            .iter()
            .filter(|t| now.duration_since(**t) < Duration::from_secs(60))
            .count();
        if in_window >= self.config.max_per_minute as usize {
            return ThrottleDecision::Throttled {
                reason: ThrottleReason::PerMinuteCap,
            };
        }
        ThrottleDecision::Fire
    }

    /// Record that a toast fired. Call AFTER a successful `Fire` decision.
    /// Updates seen_ids, last_toast, recent_toasts, and evicts expired
    /// entries to bound memory.
    pub fn record_toast(&mut self, id: Option<String>) {
        let now = Instant::now();
        if let Some(id) = id {
            self.seen_ids.insert(id, now);
            // Evict entries older than 2x dedup window (safe slack).
            self.seen_ids
                .retain(|_, t| now.duration_since(*t) < self.config.dedup_window * 2);
            // Hard cap on HashMap size as a safety net (malformed callers
            // sending random ids shouldn't grow this without bound).
            if self.seen_ids.len() > 4096 {
                let cutoff = now - self.config.dedup_window;
                self.seen_ids.retain(|_, t| *t > cutoff);
            }
        }
        self.last_toast = Some(now);
        self.recent_toasts.push_back(now);
        // Evict timestamps older than 60s — they're no longer in the
        // rolling window.
        while let Some(front) = self.recent_toasts.front() {
            if now.duration_since(*front) >= Duration::from_secs(60) {
                self.recent_toasts.pop_front();
            } else {
                break;
            }
        }
    }

    /// Total OS toasts fired in the last 60s. Useful for `help/state`-style
    /// diagnostics.
    #[allow(dead_code)]
    pub fn recent_count(&self) -> usize {
        let now = Instant::now();
        self.recent_toasts
            .iter()
            .filter(|t| now.duration_since(**t) < Duration::from_secs(60))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_id_always_fires() {
        let t = NotificationThrottle::new(ThrottleConfig::default());
        assert_eq!(t.check_toast(&None), ThrottleDecision::Fire);
    }

    #[test]
    fn duplicate_id_within_window_suppresses() {
        let mut t = NotificationThrottle::new(ThrottleConfig::default());
        let id = Some("abc".to_string());
        assert_eq!(t.check_toast(&id), ThrottleDecision::Fire);
        t.record_toast(id.clone());
        assert_eq!(t.check_toast(&id), ThrottleDecision::SuppressedDuplicate);
    }

    #[test]
    fn different_id_fires() {
        // Default config has 2s min_interval — set to 0 so the back-to-back
        // check below isn't throttled by something other than dedup.
        let mut t = NotificationThrottle::new(ThrottleConfig {
            min_interval: Duration::ZERO,
            ..ThrottleConfig::default()
        });
        t.record_toast(Some("abc".into()));
        assert_eq!(
            t.check_toast(&Some("xyz".into())),
            ThrottleDecision::Fire
        );
    }

    #[test]
    fn min_interval_throttles() {
        let mut t = NotificationThrottle::new(ThrottleConfig {
            min_interval: Duration::from_secs(10),
            ..ThrottleConfig::default()
        });
        t.record_toast(Some("a".into()));
        // Second check immediately — should be throttled by min_interval.
        assert_eq!(
            t.check_toast(&Some("b".into())),
            ThrottleDecision::Throttled {
                reason: ThrottleReason::MinInterval
            }
        );
    }

    #[test]
    fn per_minute_cap_throttles() {
        let mut t = NotificationThrottle::new(ThrottleConfig {
            max_per_minute: 3,
            min_interval: Duration::ZERO,
            dedup_window: Duration::ZERO,
            ..ThrottleConfig::default()
        });
        // 3 fires should be OK.
        for i in 0..3 {
            assert_eq!(
                t.check_toast(&Some(format!("id-{i}"))),
                ThrottleDecision::Fire
            );
            t.record_toast(Some(format!("id-{i}")));
        }
        // 4th should be throttled.
        assert_eq!(
            t.check_toast(&Some("id-3".into())),
            ThrottleDecision::Throttled {
                reason: ThrottleReason::PerMinuteCap
            }
        );
    }

    #[test]
    fn peek_does_not_mutate() {
        let mut t = NotificationThrottle::new(ThrottleConfig {
            min_interval: Duration::from_secs(10),
            ..ThrottleConfig::default()
        });
        t.record_toast(Some("a".into()));
        // Peek returns throttled but doesn't change state.
        assert_eq!(
            t.check_toast(&Some("b".into())),
            ThrottleDecision::Throttled {
                reason: ThrottleReason::MinInterval
            }
        );
        // record_toast should advance last_toast, so subsequent check is also throttled.
        t.record_toast(Some("b".into()));
        assert_eq!(
            t.check_toast(&Some("c".into())),
            ThrottleDecision::Throttled {
                reason: ThrottleReason::MinInterval
            }
        );
    }
}