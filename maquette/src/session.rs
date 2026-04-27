//! GUI session state: tracks the currently-open `.maq` file, drives
//! File → New / Open / Save / Save As through `rfd` dialogs, and
//! keeps the window title in sync.
//!
//! **GUI-only.** The pure load/save logic lives in
//! [`maquette::project`]; this module just wires it to rfd, Bevy
//! systems, and the `EditHistory`.
//!
//! Kept outside the lib so the CLI binary has nothing to do with
//! file dialogs, `Window`, or project-dirty tracking.
//!
//! ## Why the dialogs are split into two phases
//!
//! Every native file dialog goes through [`PendingProjectDialog`].
//! The old implementation called `rfd::FileDialog::pick_file()` /
//! `save_file()` synchronously from `handle_project_action`. Those
//! sync paths end in `NSOpenPanel.runModal()` / `NSSavePanel.runModal()`
//! on macOS, which nests a modal run-loop under winit's own callback
//! and — on macOS 26+ — wedges the app forever (see the matching
//! incident in `export_dialog.rs`).
//!
//! Replacing them with `rfd::AsyncFileDialog` means the dialog is
//! a future we poll every frame; Cocoa shows the panel as a sheet
//! via `beginSheetModalForWindow:completionHandler:`, which is the
//! integration pattern that actually works from inside winit's
//! event handler.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::{PrimaryWindow, RequestRedraw};
use maquette::grid::{Grid, Palette};
use maquette::project::{self, FILE_EXT};

use crate::autosave::{self, RecoveryPrompt};
use crate::history::EditHistory;
use crate::notify::Toasts;

/// Where the current in-memory canvas lives on disk (if at all) and
/// whether it has unsaved edits.
#[derive(Resource, Default)]
pub struct CurrentProject {
    pub path: Option<PathBuf>,
    pub unsaved: bool,
}

impl CurrentProject {
    pub fn display_name(&self) -> &str {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
    }

    /// Called by the UI after any edit that mutates the canvas.
    pub fn mark_dirty(&mut self) {
        if !self.unsaved {
            self.unsaved = true;
        }
    }
}

/// User-initiated project actions. The UI translates button clicks
/// into these messages; a single handler system performs the actual
/// I/O so that file dialogs and `std::fs` calls are centralized.
#[derive(Message, Clone, Copy)]
pub enum ProjectAction {
    /// Start a blank project with the given canvas dimensions.
    /// Dimensions are clamped to the supported range by `Grid::with_size`.
    New { w: usize, h: usize },
    Open,
    Save,
    SaveAs,
}

pub struct ProjectPlugin;

impl Plugin for ProjectPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentProject>()
            .init_resource::<PendingProjectDialog>()
            .add_message::<ProjectAction>()
            .add_systems(
                Update,
                (
                    handle_project_action,
                    poll_pending_project_dialog,
                    update_window_title,
                )
                    .chain(),
            );
    }
}

// ---------------------------------------------------------------------
// Pending-dialog state
// ---------------------------------------------------------------------

/// Live state for an in-flight native file dialog (Open / Save / Save
/// As). `Some` for as long as the sheet is on screen (or spawning);
/// flipped back to `None` the frame its future resolves.
///
/// We deliberately allow only one at a time — the UI already disables
/// File menu items while `is_pending()` is true, so this is the
/// last-line guard.
#[derive(Resource, Default)]
pub struct PendingProjectDialog {
    inner: Option<Pending>,
}

enum Pending {
    /// `File → Open` — pick an existing `.maq` and load it.
    Open { task: Task<Option<PathBuf>> },
    /// `File → Save As…` — always prompt for a target path.
    SaveAs { task: Task<Option<PathBuf>> },
    /// `File → Save` triggered on an untitled project — same flow as
    /// Save As, but the toast copy reads "Saved foo.maq" rather than
    /// treating it as a fresh save-to-new-location.
    SaveUntitled { task: Task<Option<PathBuf>> },
}

impl PendingProjectDialog {
    /// Is a native dialog currently up or spawning? UI uses this to
    /// disable File menu items to prevent stacking two panels.
    pub fn is_pending(&self) -> bool {
        self.inner.is_some()
    }

    fn spawn_pick(&mut self) {
        if self.inner.is_some() {
            log::warn!("project dialog: ignoring Open — a dialog is already pending");
            return;
        }
        let task = AsyncComputeTaskPool::get().spawn(async move {
            rfd::AsyncFileDialog::new()
                .add_filter("Maquette project", &[FILE_EXT])
                .pick_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        });
        self.inner = Some(Pending::Open { task });
    }

    fn spawn_save(&mut self, default_name: String, untitled: bool) {
        if self.inner.is_some() {
            log::warn!("project dialog: ignoring Save/SaveAs — a dialog is already pending");
            return;
        }
        let task = AsyncComputeTaskPool::get().spawn(async move {
            rfd::AsyncFileDialog::new()
                .add_filter("Maquette project", &[FILE_EXT])
                .set_file_name(default_name)
                .save_file()
                .await
                .map(|handle| handle.path().to_path_buf())
        });
        self.inner = Some(if untitled {
            Pending::SaveUntitled { task }
        } else {
            Pending::SaveAs { task }
        });
    }
}

// ---------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_project_action(
    mut events: MessageReader<ProjectAction>,
    mut grid: ResMut<Grid>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut history: ResMut<EditHistory>,
    mut toasts: ResMut<Toasts>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut pending: ResMut<PendingProjectDialog>,
) {
    if events.is_empty() {
        return;
    }
    // Any action either popped a native dialog or wrote a file.
    // Under `WinitSettings::desktop_app()` the event loop won't
    // auto-tick after that, so the toast / title update we emit
    // below would sit invisible until the 5 s heartbeat. One
    // explicit redraw per batch unclogs the loop.
    redraw.write(RequestRedraw);

    for action in events.read() {
        match action {
            ProjectAction::New { w, h } => {
                *grid = Grid::with_size(*w, *h);
                // File → New starts with a pristine palette. If we
                // only reset `selected`, any colors the user had
                // previously edited or deleted would carry over into
                // the new project — surprising behaviour for the
                // user.
                *palette = Palette::default();
                current.path = None;
                current.unsaved = false;
                history.clear();
            }
            ProjectAction::Open => {
                pending.spawn_pick();
            }
            ProjectAction::Save => {
                // Save with a known path is a plain `std::fs::write`
                // — never pops a dialog, so it stays on the fast
                // synchronous path.
                match current.path.clone() {
                    Some(path) => apply_save(&path, &grid, &palette, &mut current, &mut toasts),
                    None => pending.spawn_save(default_save_name(&current), /* untitled */ true),
                }
            }
            ProjectAction::SaveAs => {
                pending.spawn_save(default_save_name(&current), /* untitled */ false);
            }
        }
    }
}

/// Drain the in-flight file dialog and perform the I/O half once the
/// user commits. This is the async-flow counterpart to what the old
/// `handle_project_action` did inline right after `pick_file()` /
/// `save_file()` returned.
#[allow(clippy::too_many_arguments)]
fn poll_pending_project_dialog(
    mut pending: ResMut<PendingProjectDialog>,
    mut grid: ResMut<Grid>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut history: ResMut<EditHistory>,
    mut toasts: ResMut<Toasts>,
    mut recovery: ResMut<RecoveryPrompt>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(kind) = pending.inner.as_mut() else {
        return;
    };

    // Keep the reactive loop awake while the dialog is up, otherwise
    // the completion handler might fire during a 5 s heartbeat
    // quiet-period and we'd notice the picked path a frame late.
    redraw.write(RequestRedraw);

    // Poll the active task without moving `Pending` out of the option.
    let resolved = match kind {
        Pending::Open { task } => match block_on(future::poll_once(task)) {
            Some(result) => Resolved::Open(result),
            None => return,
        },
        Pending::SaveAs { task } => match block_on(future::poll_once(task)) {
            Some(result) => Resolved::SaveAs(result),
            None => return,
        },
        Pending::SaveUntitled { task } => match block_on(future::poll_once(task)) {
            Some(result) => Resolved::SaveUntitled(result),
            None => return,
        },
    };

    // Dialog resolved — clear the slot before the I/O runs so a
    // panic during load/save doesn't leave the state marked
    // "dialog pending forever" and freeze the File menu.
    pending.inner = None;
    // Force one more redraw so the toast / title update below
    // reaches the screen promptly.
    redraw.write(RequestRedraw);

    match resolved {
        Resolved::Open(Some(path)) => apply_open(
            &path,
            &mut grid,
            &mut palette,
            &mut current,
            &mut history,
            &mut toasts,
            &mut recovery,
        ),
        Resolved::SaveAs(Some(path)) | Resolved::SaveUntitled(Some(path)) => {
            apply_save(&path, &grid, &palette, &mut current, &mut toasts);
        }
        Resolved::Open(None) => log::info!("project dialog: Open cancelled"),
        Resolved::SaveAs(None) => log::info!("project dialog: Save As cancelled"),
        Resolved::SaveUntitled(None) => log::info!("project dialog: Save (untitled) cancelled"),
    }
}

enum Resolved {
    Open(Option<PathBuf>),
    SaveAs(Option<PathBuf>),
    SaveUntitled(Option<PathBuf>),
}

// ---------------------------------------------------------------------
// I/O helpers (pure business logic, no dialog)
// ---------------------------------------------------------------------

fn apply_open(
    path: &std::path::Path,
    grid: &mut Grid,
    palette: &mut Palette,
    current: &mut CurrentProject,
    history: &mut EditHistory,
    toasts: &mut Toasts,
    recovery: &mut RecoveryPrompt,
) {
    // Look for a newer autosave sidecar BEFORE the load. The normal
    // load proceeds either way — if the user chooses Recover, the
    // modal overwrites the grid / palette with the swap's contents.
    // Loading the `.maq` first keeps the in-between frame showing a
    // valid project instead of a blank canvas.
    let has_recovery = project::swap_is_newer(path) == Some(true);

    match project::apply_to_grid_and_palette(path, grid, palette) {
        Ok(()) => {
            let name = file_name_or(path, "project").to_string();
            let path_buf = path.to_path_buf();
            current.path = Some(path_buf.clone());
            current.unsaved = false;
            history.clear();
            toasts.success(format!("Opened {name}"));
            if has_recovery {
                autosave::arm_recovery_prompt(recovery, path_buf);
            }
        }
        Err(e) => {
            log::error!("open failed: {e}");
            toasts.error(format!("Open failed — {e}"));
        }
    }
}

fn apply_save(
    path: &std::path::Path,
    grid: &Grid,
    palette: &Palette,
    current: &mut CurrentProject,
    toasts: &mut Toasts,
) {
    match project::write_project(path, grid, palette) {
        Ok(()) => {
            let name = file_name_or(path, "project").to_string();
            current.path = Some(path.to_path_buf());
            current.unsaved = false;
            toasts.success(format!("Saved {name}"));
        }
        Err(e) => {
            log::error!("save failed: {e}");
            toasts.error(format!("Save failed — {e}"));
        }
    }
}

fn file_name_or<'a>(path: &'a std::path::Path, fallback: &'a str) -> &'a str {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback)
}

fn default_save_name(current: &CurrentProject) -> String {
    current
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| format!("untitled.{FILE_EXT}"))
}

fn update_window_title(
    current: Res<CurrentProject>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !current.is_changed() {
        return;
    }
    let prefix = if current.unsaved { "• " } else { "" };
    let name = current.display_name();
    if let Ok(mut w) = windows.single_mut() {
        w.title = format!("{prefix}{name} — Maquette");
    }
}
