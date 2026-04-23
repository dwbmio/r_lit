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

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
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
            .add_message::<ProjectAction>()
            .add_systems(
                Update,
                (handle_project_action, update_window_title).chain(),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_project_action(
    mut events: MessageReader<ProjectAction>,
    mut grid: ResMut<Grid>,
    mut palette: ResMut<Palette>,
    mut current: ResMut<CurrentProject>,
    mut history: ResMut<EditHistory>,
    mut toasts: ResMut<Toasts>,
    mut recovery: ResMut<RecoveryPrompt>,
) {
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
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("Maquette project", &[FILE_EXT])
                    .pick_file()
                else {
                    continue;
                };
                // Look for a newer autosave sidecar BEFORE the
                // load. The normal load proceeds either way — if the
                // user chooses Recover, the modal overwrites the
                // grid / palette with the swap's contents. Loading
                // the `.maq` first keeps the in-between frame
                // showing a valid project instead of a blank canvas.
                let has_recovery = project::swap_is_newer(&path) == Some(true);

                match project::apply_to_grid_and_palette(&path, &mut grid, &mut palette) {
                    Ok(()) => {
                        let name = file_name_or(&path, "project").to_string();
                        let path_clone = path.clone();
                        current.path = Some(path);
                        current.unsaved = false;
                        history.clear();
                        toasts.success(format!("Opened {name}"));
                        if has_recovery {
                            autosave::arm_recovery_prompt(&mut recovery, path_clone);
                        }
                    }
                    Err(e) => {
                        log::error!("open failed: {e}");
                        toasts.error(format!("Open failed — {e}"));
                    }
                }
            }
            ProjectAction::Save => {
                let path = match current.path.clone() {
                    Some(p) => p,
                    None => {
                        let Some(p) = prompt_save_path(&current) else {
                            continue;
                        };
                        p
                    }
                };
                match project::write_project(&path, &grid, &palette) {
                    Ok(()) => {
                        let name = file_name_or(&path, "project").to_string();
                        current.path = Some(path);
                        current.unsaved = false;
                        toasts.success(format!("Saved {name}"));
                    }
                    Err(e) => {
                        log::error!("save failed: {e}");
                        toasts.error(format!("Save failed — {e}"));
                    }
                }
            }
            ProjectAction::SaveAs => {
                let Some(path) = prompt_save_path(&current) else {
                    continue;
                };
                match project::write_project(&path, &grid, &palette) {
                    Ok(()) => {
                        let name = file_name_or(&path, "project").to_string();
                        current.path = Some(path);
                        current.unsaved = false;
                        toasts.success(format!("Saved {name}"));
                    }
                    Err(e) => {
                        log::error!("save-as failed: {e}");
                        toasts.error(format!("Save-as failed — {e}"));
                    }
                }
            }
        }
    }
}

fn file_name_or<'a>(path: &'a std::path::Path, fallback: &'a str) -> &'a str {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fallback)
}

fn prompt_save_path(current: &CurrentProject) -> Option<PathBuf> {
    let default_name = current
        .path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| format!("untitled.{FILE_EXT}"));
    rfd::FileDialog::new()
        .add_filter("Maquette project", &[FILE_EXT])
        .set_file_name(default_name)
        .save_file()
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
