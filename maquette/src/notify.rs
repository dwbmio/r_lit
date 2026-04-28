//! Toast / notification layer for the GUI.
//!
//! Previously, I/O failures in `session.rs` and `export.rs` were
//! `log::error!`-ed and never surfaced to the user. That's the
//! textbook silent-failure antipattern — a save could fail (disk
//! full, permissions, EACCES on a network mount) and the user
//! would keep painting against a stale "saved" state.
//!
//! This module lifts those errors onto the user's screen as
//! short-lived toasts in the top-right corner of the primary
//! window, color-coded by severity:
//!
//! * **Success** (green) — "Saved foo.maq", "Exported foo.glb"
//! * **Info**    (blue)  — informational only, auto-dismiss fast
//! * **Warning** (amber) — recoverable degraded state
//! * **Error**   (red)   — operation failed; message surfaces the
//!   underlying `thiserror` variant verbatim (already user-facing
//!   since v0.5).
//!
//! Toasts auto-dismiss after `DEFAULT_TTL_SECS` and can be closed
//! manually with a click. Up to `MAX_VISIBLE` render at once; older
//! ones are dropped from the queue before newer ones are inserted.
//!
//! **GUI-only.** Nothing in this module is reachable from the CLI
//! per the Headless Invariant. Lib code that needs to report a
//! condition to the user emits a message (see `ExportOutcome` in
//! `maquette::export`); the GUI translates the message into a toast.

use std::collections::VecDeque;
use std::time::Duration;

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use maquette::export::ExportOutcome;

/// How long a toast stays on screen before auto-dismissing. Chosen
/// long enough to read a two-line error, short enough not to stack
/// up during a batch operation. Errors get 2× this (see
/// `Toast::ttl_secs`) because they're the ones users actually need
/// time to act on.
const DEFAULT_TTL_SECS: f32 = 4.0;

/// Upper bound on how many toasts render simultaneously. Over-quota
/// toasts evict the oldest.
const MAX_VISIBLE: usize = 5;

/// Approximate fade-out window at the end of a toast's life, in
/// seconds. Cheap opacity ramp — no tween library needed.
const FADE_SECS: f32 = 0.5;

pub struct NotifyPlugin;

impl Plugin for NotifyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Toasts>()
            .add_systems(Update, (consume_export_outcomes, gc_expired_toasts))
            .add_systems(EguiPrimaryContextPass, render_toasts);
    }
}

#[derive(Resource, Default)]
pub struct Toasts {
    items: VecDeque<Toast>,
}

impl Toasts {
    pub fn success(&mut self, msg: impl Into<String>) {
        self.push(ToastLevel::Success, msg.into());
    }

    // Reserved for v0.9+ (autosave status, disk-low warnings, etc.).
    // Kept in the public API so callers don't need a cross-version
    // shim, and flagged dead-code-allowed so `-D warnings` stays happy.
    #[allow(dead_code)]
    pub fn info(&mut self, msg: impl Into<String>) {
        self.push(ToastLevel::Info, msg.into());
    }

    #[allow(dead_code)]
    pub fn warning(&mut self, msg: impl Into<String>) {
        self.push(ToastLevel::Warning, msg.into());
    }

    pub fn error(&mut self, msg: impl Into<String>) {
        self.push(ToastLevel::Error, msg.into());
    }

    fn push(&mut self, level: ToastLevel, message: String) {
        while self.items.len() >= MAX_VISIBLE {
            self.items.pop_front();
        }
        self.items.push_back(Toast {
            level,
            message,
            age: Duration::ZERO,
        });
    }
}

#[derive(Debug, Clone)]
struct Toast {
    level: ToastLevel,
    message: String,
    age: Duration,
}

impl Toast {
    fn ttl_secs(&self) -> f32 {
        match self.level {
            ToastLevel::Error => DEFAULT_TTL_SECS * 2.0,
            _ => DEFAULT_TTL_SECS,
        }
    }

    /// Opacity factor in [0, 1] — full for the body of the toast's
    /// life, linearly ramps down during the last `FADE_SECS`.
    fn opacity(&self) -> f32 {
        let age = self.age.as_secs_f32();
        let ttl = self.ttl_secs();
        let remaining = (ttl - age).max(0.0);
        (remaining / FADE_SECS).clamp(0.0, 1.0)
    }

    fn is_expired(&self) -> bool {
        self.age.as_secs_f32() >= self.ttl_secs()
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ToastLevel {
    Success,
    #[allow(dead_code)] // v0.9+
    Info,
    #[allow(dead_code)] // v0.9+
    Warning,
    Error,
}

impl ToastLevel {
    fn accent(self) -> egui::Color32 {
        match self {
            ToastLevel::Success => egui::Color32::from_rgb(80, 180, 120),
            ToastLevel::Info => egui::Color32::from_rgb(90, 150, 220),
            ToastLevel::Warning => egui::Color32::from_rgb(230, 180, 60),
            ToastLevel::Error => egui::Color32::from_rgb(230, 95, 95),
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            ToastLevel::Success => "✓",
            ToastLevel::Info => "i",
            ToastLevel::Warning => "!",
            ToastLevel::Error => "×",
        }
    }
}

fn consume_export_outcomes(
    mut events: MessageReader<ExportOutcome>,
    mut toasts: ResMut<Toasts>,
) {
    for ev in events.read() {
        match ev {
            ExportOutcome::Success { path } => {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("output");
                toasts.success(format!("Exported {name}"));
            }
            ExportOutcome::Failure { message } => {
                toasts.error(format!("Export failed — {message}"));
            }
        }
    }
}

fn gc_expired_toasts(time: Res<Time>, mut toasts: ResMut<Toasts>) {
    let dt = time.delta();
    for t in &mut toasts.items {
        t.age += dt;
    }
    while toasts.items.front().is_some_and(Toast::is_expired) {
        toasts.items.pop_front();
    }
}

fn render_toasts(
    mut ctx: EguiContexts,
    mut toasts: ResMut<Toasts>,
) -> Result {
    if toasts.items.is_empty() {
        return Ok(());
    }
    let ctx = ctx.ctx_mut()?;

    // Right-align toasts to the *central* (non-panel) area so they
    // don't sit on top of the right SidePanel (Block Library, added
    // in v0.10 C-2). `available_rect` here reflects everything that
    // SidePanel/TopBottomPanel calls have claimed by the time
    // `render_toasts` runs (it's chained after `ui_system`).
    let central = ctx.available_rect();
    let toast_right_x = central.max.x - 12.0;
    let toast_first_y = central.min.y + 90.0;
    let mut dismiss_indices: Vec<usize> = Vec::new();
    let mut y_offset = 0.0_f32;

    for (i, toast) in toasts.items.iter().enumerate() {
        let opacity = toast.opacity();
        if opacity <= 0.0 {
            continue;
        }

        let id = egui::Id::new(("toast", i));
        let toast_pos = egui::pos2(toast_right_x, toast_first_y + y_offset);
        let area = egui::Area::new(id)
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(toast_pos)
            .interactable(true)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                let accent = apply_opacity(toast.level.accent(), opacity);
                let bg = apply_opacity(egui::Color32::from_rgb(24, 26, 30), opacity * 0.95);
                let text_color = apply_opacity(egui::Color32::from_gray(235), opacity);

                egui::Frame::new()
                    .fill(bg)
                    .stroke(egui::Stroke::new(1.5, accent))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(toast.level.glyph())
                                    .strong()
                                    .color(accent)
                                    .size(14.0),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&toast.message)
                                        .color(text_color)
                                        .size(12.0),
                                )
                                .wrap()
                                .truncate(),
                            );
                        })
                        .response
                    })
            });

        // Single click anywhere on the toast dismisses it.
        if area.inner.inner.clicked() {
            dismiss_indices.push(i);
        }

        y_offset += 46.0;
    }

    // Remove dismissed in reverse so indices stay valid.
    for i in dismiss_indices.into_iter().rev() {
        if i < toasts.items.len() {
            toasts.items.remove(i);
        }
    }

    Ok(())
}

fn apply_opacity(color: egui::Color32, opacity: f32) -> egui::Color32 {
    let o = opacity.clamp(0.0, 1.0);
    let [r, g, b, a] = color.to_array();
    egui::Color32::from_rgba_unmultiplied(r, g, b, (a as f32 * o) as u8)
}
