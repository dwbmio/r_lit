//! Autosave + crash recovery (v0.9 A, GUI-only).
//!
//! The model is deliberately simple:
//!
//! * While a project has a `path` (i.e. it's been saved at least
//!   once) **and** has unsaved edits, the autosave system writes a
//!   sidecar `.maq.swap` beside the project file every time a
//!   stroke is committed to `EditHistory`, and every time the
//!   window loses focus. Detection uses
//!   [`EditHistory::strokes_committed`] as a plain monotonic counter
//!   (no Bevy `Message` plumbing needed), so the history data
//!   structure stays headless-friendly.
//!
//! * A successful `Save` / `Save As` deletes the swap, since the
//!   authoritative `.maq` is now at least as fresh.
//!
//! * On `File → Open`, if the picked path has a `.maq.swap` whose
//!   mtime is strictly newer than its parent, the GUI shows a modal
//!   and offers Recover (loads the swap, marks it dirty so the
//!   next flush refreshes the sidecar, user saves explicitly) or
//!   Discard (keeps the saved `.maq`, deletes the swap). There's no
//!   third option — both outcomes leave the user with exactly one
//!   authoritative file on disk.
//!
//! ## Scope cuts (documented, intentional)
//!
//! * **Untitled projects are not autosaved.** The swap lives beside
//!   the parent; without a parent path we have nowhere natural to
//!   put it. The full solution ties into prefs (`~/.config/maquette/
//!   untitled.maq.swap`), which lands in v0.9 C. Autosave will start
//!   working for untitled sessions then.
//! * **Startup auto-recovery** (no Open needed) also requires
//!   last-opened-path persistence from v0.9 C. For now recovery
//!   triggers on Open.
//! * Writes are **not debounced**. A committed stroke is already a
//!   human-scale event (~1/sec at worst); the write itself is a
//!   single `std::fs::write` with pretty-printed JSON (~10 KB for
//!   typical projects, << 1 ms on SSD). If future telemetry shows
//!   this is hot, debounce at that point.
//!
//! ## Protocol for lib callers
//!
//! The swap file is a bit-for-bit project file. If new project-file
//! fields are added, they automatically flow through
//! [`maquette::project::write_swap`] with no extra work. If the
//! schema version bumps, swaps written by the old build remain
//! readable by the new build (same rules as `.maq`, see
//! `project.rs` schema history).

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::WindowFocused;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use maquette::grid::{Grid, Palette};
use maquette::project::{self, ProjectMeta};

use crate::history::EditHistory;
use crate::notify::Toasts;
use crate::session::CurrentProject;

pub struct AutosavePlugin;

impl Plugin for AutosavePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AutosaveState>()
            .init_resource::<RecoveryPrompt>()
            .add_systems(
                Update,
                (autosave_on_stroke, autosave_on_blur, cleanup_swap_on_save)
                    // Run in this order so a stroke-closed + save
                    // in the same frame (rare but possible via
                    // keyboard-driven paint + Cmd+S) ends with the
                    // swap deleted, not re-created.
                    .chain(),
            )
            .add_systems(EguiPrimaryContextPass, render_recovery_modal);
    }
}

/// Mutable bookkeeping for the autosave loop.
#[derive(Resource, Default)]
pub struct AutosaveState {
    /// Last value of `EditHistory::strokes_committed` we flushed at.
    /// A delta from this = "something new to autosave".
    last_flushed_strokes: u64,
    /// Path whose swap we currently track. Reset to `None` when the
    /// user switches projects; that way Save-As to a new location
    /// doesn't leave an orphan swap next to the old one.
    tracked: Option<PathBuf>,
}

/// Resource that drives the recovery modal's visibility. `session.rs`
/// arms it via [`arm_recovery_prompt`] after an Open that found a
/// newer swap; the modal renders until the user picks one of the
/// two outcomes.
#[derive(Resource, Default)]
pub struct RecoveryPrompt {
    pending: Option<PendingRecovery>,
}

struct PendingRecovery {
    project_path: PathBuf,
}

// ---------------------------------------------------------------------
// Flush systems
// ---------------------------------------------------------------------

fn autosave_on_stroke(
    history: Res<EditHistory>,
    grid: Res<Grid>,
    palette: Res<Palette>,
    meta: Res<ProjectMeta>,
    current: Res<CurrentProject>,
    mut state: ResMut<AutosaveState>,
) {
    let committed = history.strokes_committed();

    // Keep `tracked` aligned with the current project, resetting our
    // baseline each time the user opens / creates a different file.
    // This avoids a spurious write right after Open that would churn
    // the freshly-recovered swap.
    if current.path.as_ref() != state.tracked.as_ref() {
        state.tracked = current.path.clone();
        state.last_flushed_strokes = committed;
        return;
    }

    if committed == state.last_flushed_strokes {
        return;
    }

    let Some(path) = current.path.as_ref() else {
        // Untitled — scope-cut, see module doc.
        state.last_flushed_strokes = committed;
        return;
    };
    if !current.unsaved {
        // Only unsaved edits need a swap; a clean project's
        // authoritative copy is already on disk.
        state.last_flushed_strokes = committed;
        return;
    }

    flush_swap(path, &grid, &palette, &meta, &mut state, committed);
}

fn autosave_on_blur(
    mut events: MessageReader<WindowFocused>,
    history: Res<EditHistory>,
    grid: Res<Grid>,
    palette: Res<Palette>,
    meta: Res<ProjectMeta>,
    current: Res<CurrentProject>,
    mut state: ResMut<AutosaveState>,
) {
    // Coalesce: one blur per frame is enough even if multiple
    // windows report focus changes (float preview + primary).
    let lost_focus = events.read().any(|ev| !ev.focused);
    if !lost_focus {
        return;
    }
    let Some(path) = current.path.as_ref() else {
        return;
    };
    if !current.unsaved {
        return;
    }
    let committed = history.strokes_committed();
    if committed == state.last_flushed_strokes {
        // Nothing new since the last flush — skip. Blur alone isn't
        // a write trigger; it's "write pending edits now, if any".
        return;
    }
    flush_swap(path, &grid, &palette, &meta, &mut state, committed);
}

fn flush_swap(
    project_path: &std::path::Path,
    grid: &Grid,
    palette: &Palette,
    meta: &ProjectMeta,
    state: &mut AutosaveState,
    committed: u64,
) {
    // v0.10 D-1 migration: was `project::write_swap` (which goes
    // through the legacy `write_project`, dropping ProjectMeta on
    // the floor). Now `write_project_with_meta(swap_path(...), …)`
    // — a typed `model_description` set in the GUI survives an
    // autosave + crash + Recover cycle.
    let swap_path = project::swap_path(project_path);
    match project::write_project_with_meta(&swap_path, grid, palette, meta) {
        Ok(()) => {
            state.last_flushed_strokes = committed;
            log::info!("autosaved → {}", swap_path.display());
        }
        Err(e) => {
            // We intentionally don't toast here — autosave is
            // supposed to be invisible. A persistent failure will
            // show up as an explicit save failing at Cmd+S. A
            // single log line per flush is enough signal in stderr.
            log::warn!("autosave failed: {e}");
        }
    }
}

fn cleanup_swap_on_save(current: Res<CurrentProject>, mut state: ResMut<AutosaveState>) {
    // A successful Save / Save As leaves `unsaved = false` AND a
    // path set. When we observe that transition, remove the
    // sidecar: the parent `.maq` is now at least as fresh as the
    // swap, so a recovery modal next launch would only be noise.
    if !current.is_changed() {
        return;
    }
    if current.unsaved {
        return;
    }
    let Some(path) = current.path.as_ref() else {
        return;
    };
    if let Err(e) = project::remove_swap(path) {
        log::warn!("failed to clean up swap: {e}");
    }
    // Re-baseline: we just saved, so next stroke is the one that
    // matters. Without this, the very next `autosave_on_stroke`
    // tick would write a swap identical to the `.maq` we just
    // wrote.
    state.tracked = Some(path.clone());
}

// ---------------------------------------------------------------------
// Recovery-modal plumbing
// ---------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn render_recovery_modal(
    mut ctx: EguiContexts,
    mut prompt: ResMut<RecoveryPrompt>,
    mut grid: ResMut<Grid>,
    mut palette: ResMut<Palette>,
    mut meta: ResMut<ProjectMeta>,
    mut current: ResMut<CurrentProject>,
    mut history: ResMut<EditHistory>,
    mut toasts: ResMut<Toasts>,
    mut state: ResMut<AutosaveState>,
) -> Result {
    let Some(pending) = prompt.pending.as_ref() else {
        return Ok(());
    };
    let project_path = pending.project_path.clone();
    let ctx = ctx.ctx_mut()?;

    let mut decision: Option<Decision> = None;

    egui::Window::new("Recover unsaved changes?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            let name = project_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("this project");
            ui.set_min_width(360.0);
            ui.label(format!(
                "An autosave sidecar was found beside {name} that is newer than the saved file."
            ));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(
                    "This usually means the editor closed without a clean save — \
                     maybe a crash, or you force-quit.",
                )
                .color(egui::Color32::from_gray(180)),
            );
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Recover unsaved edits").clicked() {
                    decision = Some(Decision::Recover);
                }
                if ui.button("Discard swap and open saved file").clicked() {
                    decision = Some(Decision::Discard);
                }
            });
        });

    let Some(decision) = decision else {
        return Ok(());
    };

    match decision {
        Decision::Recover => {
            // v0.10 D-1: was `read_project` which dropped meta.
            // Now `read_project_with_meta` so a recovered project
            // surfaces the same `model_description` /
            // `texture_prefs` it had before the crash.
            match project::read_project_with_meta(&project::swap_path(&project_path)) {
                Ok((g, p, m)) => {
                    *grid = g;
                    *palette = p;
                    *meta = m;
                    current.path = Some(project_path.clone());
                    // The in-memory state is now strictly newer than
                    // the `.maq` — mark dirty so the user sees the
                    // unsaved indicator in the title bar and the next
                    // autosave tick refreshes the swap.
                    current.unsaved = true;
                    history.clear();
                    // Re-baseline autosave so its first tick after
                    // recovery doesn't immediately rewrite the swap.
                    state.tracked = Some(project_path.clone());
                    state.last_flushed_strokes = history.strokes_committed();
                    toasts.success("Recovered unsaved changes");
                }
                Err(e) => {
                    toasts.error(format!("Recovery failed — {e}"));
                }
            }
        }
        Decision::Discard => {
            if let Err(e) = project::remove_swap(&project_path) {
                log::warn!("failed to delete discarded swap: {e}");
            }
            toasts.info("Discarded autosave; opened saved file");
            // The normal Open flow in session.rs already loaded the
            // `.maq`, so no additional work here.
        }
    }

    prompt.pending = None;
    Ok(())
}

enum Decision {
    Recover,
    Discard,
}

/// Public entry point: arms the recovery modal for `project_path` on
/// the next frame. `session.rs` calls this after a successful Open
/// when it detects a newer swap.
pub fn arm_recovery_prompt(prompt: &mut RecoveryPrompt, project_path: PathBuf) {
    prompt.pending = Some(PendingRecovery { project_path });
}
