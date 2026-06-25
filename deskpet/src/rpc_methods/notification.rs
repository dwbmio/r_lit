//! `notification/*` methods. Migrate the old `Incoming::Notify` /
//! `Incoming::Clear` messages to typed JSON-RPC, and add `notification/list`
//! for state inspection.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::notify::{make_notice, Level, Notice, NotificationAction, NotifyState};
use crate::os_notify::{NotificationThrottle, ThrottleDecision};
use crate::rpc::{Method, RpcError};
use serde_json::Value;

// ---- notification/show -----------------------------------------------------

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ShowParams {
    /// Reminder body. Required.
    pub body: String,
    /// Optional bold title line.
    #[serde(default)]
    pub title: Option<String>,
    /// `info` (default) | `success` | `warn` | `error`. Drives bubble accent.
    #[serde(default)]
    pub level: Option<String>,
    /// How long to keep it visible (ms). Default derives from level + body length.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Client-supplied correlation id (e.g., a UUID). When set, the OS-toast
    /// layer dedupes: same id within the dedup window is suppressed
    /// (returns `suppressed=true`). Bubbles are NOT deduped — every
    /// notice shows. Use for retry semantics ("don't toast again, the
    /// bubble is enough").
    #[serde(default)]
    pub id: Option<String>,
    /// Drop the notice if it hasn't been shown within this many ms from
    /// when the call landed. Use for background-task notifications that
    /// become irrelevant after a delay (e.g., "build finished 10 min
    /// ago" — don't pop up after the user has moved on). Default: no
    /// expiry.
    #[serde(default)]
    pub expires_in_ms: Option<u64>,
    /// Action buttons shown in the OS notification. v1: clicks are logged
    /// but no callback is delivered. macOS only shows the first action
    /// (NSUserNotification limitation); Windows shows up to OS limits.
    #[serde(default)]
    pub actions: Vec<NotificationAction>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ShowResult {
    /// `true` if the reminder is on screen now, `false` if it was queued
    /// behind an existing reminder.
    pub shown: bool,
    /// Whether a previous reminder was already on screen at the time of
    /// the call (the new one was queued).
    pub queued: bool,
    /// `true` if the message was dropped due to dedup (same `id` within
    /// the dedup window). The bubble is unaffected; only the OS toast
    /// is suppressed.
    #[serde(default)]
    pub suppressed: bool,
    /// Echoed back from `params.id` so the client can correlate without
    /// re-parsing. `null` when no id was supplied.
    #[serde(default)]
    pub id: Option<String>,
}

pub struct ShowMethod;

impl Method for ShowMethod {
    fn name(&self) -> &'static str {
        "notification/show"
    }
    fn description(&self) -> &'static str {
        "Show a reminder above the mascot. If a reminder is already on \
         screen, the new one is queued and shown after it expires."
    }
    fn invoke(&self, world: &mut World, params: Value) -> Result<Value, RpcError> {
        let p: ShowParams = serde_json::from_value(params)
            .map_err(|e| RpcError::InvalidParams(e.to_string()))?;
        if p.body.trim().is_empty() {
            return Err(RpcError::InvalidParams("body must be non-empty".into()));
        }

        let level: Level = match p.level.as_deref() {
            None | Some("") | Some("info") => Level::Info,
            Some("success") => Level::Success,
            Some("warn") | Some("warning") => Level::Warn,
            Some("error") | Some("err") => Level::Error,
            Some(other) => {
                return Err(RpcError::InvalidParams(format!(
                    "unknown level '{other}' (expected info|success|warn|error)"
                )));
            }
        };

        // Peek the throttle so we can tell the caller whether the OS toast will
        // be suppressed (due to dedup). This is a *peek* — does not mutate
        // the throttle state. The dispatcher will perform the actual
        // record_toast() after firing.
        let suppressed: bool = world
            .get_resource::<NotificationThrottle>()
            .map(|t: &NotificationThrottle| !matches!(t.check_toast(&p.id), ThrottleDecision::Fire))
            .unwrap_or(false);

        let mut state = world
            .get_resource_mut::<NotifyState>()
            .ok_or_else(|| RpcError::Internal("NotifyState resource missing".into()))?;

        let notice = make_notice(
            p.title,
            p.body,
            level,
            p.duration_ms,
            p.id.clone(),
            p.expires_in_ms,
            p.actions,
        );
        let queued = state.showing();
        if queued {
            state.queue.push_back(notice);
        } else {
            state.current = Some(notice);
        }
        drop(state);

        // Make the window visible — same effect as the legacy `Incoming::Notify`.
        if let Ok(mut window) = world
            .query_filtered::<&mut bevy::window::Window, With<PrimaryWindow>>()
            .single_mut(world)
        {
            window.visible = true;
        }

        Ok(serde_json::to_value(ShowResult {
            shown: !queued,
            queued,
            suppressed,
            id: p.id,
        })
        .map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

/// OpenAPI stub for `notification/show`. Body is never called — the
/// `#[utoipa::path]` macro just needs a fn with the right signature.
#[allow(dead_code)]
#[utoipa::path(
    post,
    path = "/m/notification/show",
    tag = "notification",
    request_body = ShowParams,
    responses(
        (status = 200, description = "Notification accepted", body = ShowResult),
        (status = 400, description = "Invalid params"),
    ),
)]
pub fn show_path(_params: ShowParams) -> ShowResult {
    unimplemented!("openapi stub")
}

// ---- notification/clear ----------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct ClearResult {
    /// How many queued reminders were dropped.
    pub dropped: usize,
}

pub struct ClearMethod;

impl Method for ClearMethod {
    fn name(&self) -> &'static str {
        "notification/clear"
    }
    fn description(&self) -> &'static str {
        "Dismiss the current reminder and drop all queued ones."
    }
    fn invoke(&self, world: &mut World, _params: Value) -> Result<Value, RpcError> {
        let mut state = world
            .get_resource_mut::<NotifyState>()
            .ok_or_else(|| RpcError::Internal("NotifyState resource missing".into()))?;
        let dropped = state.queue.len();
        state.dismiss();
        state.queue.clear();
        Ok(serde_json::to_value(ClearResult { dropped })
            .map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

#[utoipa::path(
    post,
    path = "/m/notification/clear",
    tag = "notification",
    responses(
        (status = 200, description = "Cleared", body = ClearResult),
    ),
)]
#[allow(dead_code)]
pub fn clear_path() -> ClearResult {
    unimplemented!("openapi stub")
}

// ---- notification/list -----------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct NoticeView {
    pub title: Option<String>,
    pub body: String,
    pub level: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ListResult {
    pub current: Option<NoticeView>,
    pub queue: Vec<NoticeView>,
}

pub struct ListMethod;

impl Method for ListMethod {
    fn name(&self) -> &'static str {
        "notification/list"
    }
    fn description(&self) -> &'static str {
        "Return the on-screen reminder (if any) and the pending queue."
    }
    fn invoke(&self, world: &mut World, _params: Value) -> Result<Value, RpcError> {
        let state = world
            .get_resource::<NotifyState>()
            .ok_or_else(|| RpcError::Internal("NotifyState resource missing".into()))?;
        let view = |n: &Notice| NoticeView {
            title: n.title.clone(),
            body: n.body.clone(),
            level: format!("{:?}", n.level).to_lowercase(),
        };
        let result = ListResult {
            current: state.current.as_ref().map(view),
            queue: state.queue.iter().map(view).collect(),
        };
        Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

#[utoipa::path(
    post,
    path = "/m/notification/list",
    tag = "notification",
    responses(
        (status = 200, description = "Current + queue", body = ListResult),
    ),
)]
#[allow(dead_code)]
pub fn list_path() -> ListResult {
    unimplemented!("openapi stub")
}