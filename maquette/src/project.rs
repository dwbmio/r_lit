//! `.maq` project file format (pure / headless).
//!
//! Everything in this module is deliberately window-free so the
//! same load/save code can be driven by the GUI (through
//! `session::ProjectPlugin` in the binary) and by the CLI
//! (`maquette-cli`) and by tests. File dialogs and the Bevy-resource
//! wrapper (`CurrentProject`) live in the GUI binary; this module
//! just reads and writes bytes.
//!
//! ## File format · schema v3 (JSON)
//!
//! ```json
//! {
//!   "version": 3,
//!   "grid": { "w": 16, "h": 16, "cells": [ { "color_idx": 3, "height": 1 }, ... ] },
//!   "selected_color": 3,
//!   "palette": [ { "r": 0.9, "g": 0.3, "b": 0.35, "a": 1.0 }, null, ... ]
//! }
//! ```
//!
//! Schema history:
//! * `1` — grid + selected_color. No palette. (v0.2)
//! * `2` — adds inline `palette` field as `Vec<Rgba>`. v1 files still
//!   load; missing palette falls back to the default palette. (v0.3)
//! * `3` — palette becomes **sparse**: `Vec<Option<Rgba>>`. `null`
//!   entries represent deleted slots, preserving the slot index of
//!   every other color across palette edits. v2 files (dense, no
//!   nulls) still load because `Vec<Option<T>>` happily deserializes
//!   a no-nulls JSON array. (v0.6)

use std::path::{Path, PathBuf};

use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::grid::{Cell, Grid, Palette};

/// File extension used for Maquette project files.
pub const FILE_EXT: &str = "maq";

/// Suffix appended to a `.maq` to form its autosave sidecar. Chosen
/// over an in-place replacement (e.g. `foo.swap`) so that swap files
/// always sit next to their parent project, are visible to the user
/// in Finder / ls, and survive `find . -name '*.maq'` unless the user
/// opts into `*.maq.swap` too.
pub const SWAP_SUFFIX: &str = ".swap";

/// Schema version the exporter currently writes.
pub const SCHEMA_VERSION: u32 = 3;

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    version: u32,
    grid: GridPayload,
    selected_color: u8,
    /// Absent in v1 files; present from v2 onward. v3 allows `null`
    /// entries for deleted slots so palette indices stay stable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette: Option<Vec<Option<RgbaPayload>>>,
}

#[derive(Serialize, Deserialize)]
struct GridPayload {
    w: usize,
    h: usize,
    cells: Vec<Cell>,
}

/// On-disk color record. sRGBA 0..1 floats — stable across Bevy
/// versions and readable when a user inspects the `.maq` file by hand.
#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
struct RgbaPayload {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl From<Color> for RgbaPayload {
    fn from(c: Color) -> Self {
        let s = c.to_srgba();
        Self {
            r: s.red,
            g: s.green,
            b: s.blue,
            a: s.alpha,
        }
    }
}

impl From<&RgbaPayload> for Color {
    fn from(p: &RgbaPayload) -> Color {
        Color::srgba(p.r, p.g, p.b, p.a)
    }
}

/// Errors produced by [`read_project`] / [`apply_to_grid_and_palette`].
///
/// The public CLI displays these directly; any user-facing phrasing
/// tweaks should happen here (not in the CLI binary), so the message
/// stays consistent regardless of entry point.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid project json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported schema version {0} (this build supports up to {SCHEMA_VERSION})")]
    UnsupportedVersion(u32),
    #[error("cell count mismatch: expected {expected}, got {actual}")]
    InvalidCellCount { expected: usize, actual: usize },
}

impl ProjectFile {
    fn snapshot(grid: &Grid, palette: &Palette) -> Self {
        Self {
            version: SCHEMA_VERSION,
            grid: GridPayload {
                w: grid.w,
                h: grid.h,
                cells: grid.cells.clone(),
            },
            selected_color: palette.selected,
            palette: Some(
                palette
                    .colors
                    .iter()
                    .map(|slot| slot.map(RgbaPayload::from))
                    .collect(),
            ),
        }
    }

    fn apply_to(self, grid: &mut Grid, palette: &mut Palette) -> Result<(), ProjectError> {
        if self.version == 0 || self.version > SCHEMA_VERSION {
            return Err(ProjectError::UnsupportedVersion(self.version));
        }
        let expected = self.grid.w * self.grid.h;
        if self.grid.cells.len() != expected {
            return Err(ProjectError::InvalidCellCount {
                expected,
                actual: self.grid.cells.len(),
            });
        }
        grid.w = self.grid.w;
        grid.h = self.grid.h;
        grid.cells = self.grid.cells;
        grid.dirty = true;

        // v2 and v3: restore saved palette. v1: fall back to the
        // default palette so the file still opens cleanly.
        if let Some(slots) = self.palette {
            if !slots.is_empty() {
                palette.colors = slots
                    .iter()
                    .map(|slot| slot.as_ref().map(Color::from))
                    .collect();
            }
        } else {
            *palette = Palette::default();
        }

        // Clamp `selected_color` to a live slot. If the saved index
        // happens to point at a deleted slot (possible after a
        // palette edit + save in an older build), jump to the first
        // live color so the UI never starts up with a dead selection.
        palette.selected = if palette.is_live(self.selected_color) {
            self.selected_color
        } else {
            palette.iter_live().next().map(|(i, _)| i).unwrap_or(0)
        };
        Ok(())
    }
}

/// Read a `.maq` file from disk and return a fresh `(Grid, Palette)`.
///
/// This is the headless-friendly entry point the CLI calls directly.
/// The GUI uses [`apply_to_grid_and_palette`] against pre-existing
/// `ResMut<Grid>` / `ResMut<Palette>` handles instead.
pub fn read_project(path: &Path) -> Result<(Grid, Palette), ProjectError> {
    let text = std::fs::read_to_string(path)?;
    let pf: ProjectFile = serde_json::from_str(&text)?;
    let mut grid = Grid::with_size(pf.grid.w, pf.grid.h);
    let mut palette = Palette::default();
    pf.apply_to(&mut grid, &mut palette)?;
    Ok((grid, palette))
}

/// Mutate an existing `Grid` and `Palette` to match the project at
/// `path`. Used by the GUI's `File → Open` path where it needs to
/// keep the existing `ResMut` handles stable.
pub fn apply_to_grid_and_palette(
    path: &Path,
    grid: &mut Grid,
    palette: &mut Palette,
) -> Result<(), ProjectError> {
    let text = std::fs::read_to_string(path)?;
    let pf: ProjectFile = serde_json::from_str(&text)?;
    pf.apply_to(grid, palette)
}

/// Write the given `Grid` + `Palette` to `path` as pretty-printed JSON.
pub fn write_project(path: &Path, grid: &Grid, palette: &Palette) -> Result<(), ProjectError> {
    let pf = ProjectFile::snapshot(grid, palette);
    let text = serde_json::to_string_pretty(&pf)?;
    std::fs::write(path, text)?;
    Ok(())
}

// =====================================================================
// Autosave sidecar (v0.9 A)
// =====================================================================
//
// A `.maq.swap` is a byte-for-byte project file, written beside the
// parent `.maq` as each stroke closes. On next launch the GUI inspects
// it through `swap_is_newer` and, if so, offers recovery. The swap
// format intentionally matches `.maq` so the CLI can be pointed at a
// swap file directly (`maquette-cli info foo.maq.swap`) without a new
// verb — recovery tooling for free.

/// Return the autosave sidecar path for `project_path`
/// (`foo.maq` → `foo.maq.swap`). Works on paths without a `.maq`
/// extension too — the suffix is simply appended.
pub fn swap_path(project_path: &Path) -> PathBuf {
    let mut s = project_path.as_os_str().to_owned();
    s.push(SWAP_SUFFIX);
    PathBuf::from(s)
}

/// `Some(true)` if a swap exists whose mtime is strictly newer than
/// the parent project; `Some(false)` if the swap is stale; `None` if
/// there's no swap at all (or any stat call fails — treat as
/// "nothing to recover" rather than a hard error).
///
/// The "parent missing" case is treated as swap-wins: if the user
/// points Open at a path that no longer exists but whose swap does,
/// that's a legitimate recovery target.
pub fn swap_is_newer(project_path: &Path) -> Option<bool> {
    let swap = swap_path(project_path);
    let swap_mtime = std::fs::metadata(&swap).and_then(|m| m.modified()).ok()?;
    match std::fs::metadata(project_path).and_then(|m| m.modified()) {
        Ok(project_mtime) => Some(swap_mtime > project_mtime),
        Err(_) => Some(true),
    }
}

/// Write the swap sidecar for `project_path`. Equivalent to
/// `write_project(swap_path(project_path), …)` but names the intent
/// at the call site. Fails softly if the parent directory is gone —
/// the autosave system just tries again next stroke.
pub fn write_swap(project_path: &Path, grid: &Grid, palette: &Palette) -> Result<(), ProjectError> {
    write_project(&swap_path(project_path), grid, palette)
}

/// Delete the swap sidecar if it exists. `NotFound` is silently
/// swallowed — "already gone" is the desired post-condition.
pub fn remove_swap(project_path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(swap_path(project_path)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_reload_round_trip() {
        let dir = tempdir();
        let path = dir.join("round_trip.maq");

        let mut grid = Grid::with_size(8, 6);
        grid.paint(2, 3, 0, 1);
        grid.paint(3, 3, 1, 4);
        let palette = Palette {
            selected: 5,
            ..Palette::default()
        };

        write_project(&path, &grid, &palette).unwrap();
        let (g2, p2) = read_project(&path).unwrap();

        assert_eq!(g2.w, 8);
        assert_eq!(g2.h, 6);
        assert_eq!(g2.cells, grid.cells);
        assert_eq!(p2.selected, 5);
        assert_eq!(p2.colors.len(), palette.colors.len());
    }

    #[test]
    fn v1_file_without_palette_field_still_loads() {
        let dir = tempdir();
        let path = dir.join("v1.maq");
        let v1 = serde_json::json!({
            "version": 1,
            "grid": { "w": 2, "h": 2, "cells": [
                { "color_idx": 0, "height": 1 },
                { "color_idx": null, "height": 0 },
                { "color_idx": null, "height": 0 },
                { "color_idx": null, "height": 0 },
            ]},
            "selected_color": 2
        });
        std::fs::write(&path, v1.to_string()).unwrap();

        let (g, p) = read_project(&path).unwrap();
        assert_eq!(g.w, 2);
        assert_eq!(p.colors, Palette::default().colors);
    }

    #[test]
    fn v2_dense_palette_still_loads_in_v3_build() {
        // v2 files wrote palette as a dense `Vec<Rgba>` (no nulls).
        // Our v3 deserializer accepts that shape — every entry is
        // treated as `Some(color)`.
        let dir = tempdir();
        let path = dir.join("v2.maq");
        let v2 = serde_json::json!({
            "version": 2,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": 0, "height": 1 } ] },
            "selected_color": 0,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                { "r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0 }
            ]
        });
        std::fs::write(&path, v2.to_string()).unwrap();
        let (_, p) = read_project(&path).unwrap();
        assert_eq!(p.live_count(), 2);
        assert!(p.is_live(0));
        assert!(p.is_live(1));
    }

    #[test]
    fn v3_sparse_palette_round_trips_deleted_slots() {
        let dir = tempdir();
        let path = dir.join("sparse.maq");
        let mut grid = Grid::with_size(1, 1);
        grid.paint(0, 0, 2, 1);
        let mut palette = Palette::default();
        palette.delete(5, &mut grid, crate::grid::DeleteColorMode::Erase);
        write_project(&path, &grid, &palette).unwrap();

        // Round trip: the deleted slot must come back as `None`,
        // and every live slot must keep its exact index.
        let (_, p2) = read_project(&path).unwrap();
        assert!(!p2.is_live(5), "deleted slot must stay deleted");
        assert!(p2.is_live(0));
        assert!(p2.is_live(6));
        assert_eq!(p2.colors.len(), palette.colors.len());
    }

    #[test]
    fn selected_color_pointing_at_deleted_slot_is_remapped_on_load() {
        // Forge a file where `selected_color` refers to a null slot.
        // This shouldn't happen via the UI (delete() snaps selection
        // itself), but the loader must be defensive — the file could
        // have been hand-edited or produced by a future/buggy build.
        let dir = tempdir();
        let path = dir.join("dead_selection.maq");
        let payload = serde_json::json!({
            "version": 3,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": null, "height": 0 } ] },
            "selected_color": 1,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                null,
                { "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 }
            ]
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        let (_, p) = read_project(&path).unwrap();
        assert_eq!(p.selected, 0, "should snap to first live slot");
    }

    #[test]
    fn future_version_is_rejected() {
        let dir = tempdir();
        let path = dir.join("future.maq");
        let payload = serde_json::json!({
            "version": 9999, "grid": {"w":1,"h":1,"cells":[{"color_idx":null,"height":0}]},
            "selected_color": 0
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        assert!(matches!(
            read_project(&path),
            Err(ProjectError::UnsupportedVersion(9999))
        ));
    }

    #[test]
    fn cell_count_mismatch_is_rejected() {
        let dir = tempdir();
        let path = dir.join("bad_cells.maq");
        let payload = serde_json::json!({
            "version": 3,
            "grid": { "w": 2, "h": 2, "cells": [ {"color_idx": null, "height": 0} ] },
            "selected_color": 0
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        assert!(matches!(
            read_project(&path),
            Err(ProjectError::InvalidCellCount { expected: 4, actual: 1 })
        ));
    }

    fn tempdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "maquette_project_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // --- v0.9 A: autosave sidecar ---

    #[test]
    fn swap_path_appends_suffix() {
        let base = std::path::Path::new("/tmp/foo.maq");
        assert_eq!(swap_path(base), std::path::PathBuf::from("/tmp/foo.maq.swap"));

        // Also works on extension-less paths — swap is always a
        // strict suffix, never an extension replacement.
        let no_ext = std::path::Path::new("/tmp/untitled");
        assert_eq!(
            swap_path(no_ext),
            std::path::PathBuf::from("/tmp/untitled.swap")
        );
    }

    #[test]
    fn swap_is_newer_reports_none_when_swap_missing() {
        let dir = tempdir();
        let path = dir.join("no_swap.maq");
        let (grid, palette) = (Grid::with_size(2, 2), Palette::default());
        write_project(&path, &grid, &palette).unwrap();
        assert_eq!(swap_is_newer(&path), None);
    }

    #[test]
    fn swap_is_newer_detects_fresh_swap() {
        let dir = tempdir();
        let path = dir.join("stale_parent.maq");
        let (grid, palette) = (Grid::with_size(2, 2), Palette::default());
        write_project(&path, &grid, &palette).unwrap();

        // Give the filesystem some mtime headroom. Most filesystems
        // have millisecond or better resolution; a 50ms sleep is
        // generous but cheap.
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_swap(&path, &grid, &palette).unwrap();

        assert_eq!(
            swap_is_newer(&path),
            Some(true),
            "swap written after the project should be flagged as recoverable"
        );
    }

    #[test]
    fn swap_is_newer_reports_false_when_swap_older_than_project() {
        let dir = tempdir();
        let path = dir.join("fresh_parent.maq");
        let (grid, palette) = (Grid::with_size(2, 2), Palette::default());

        write_swap(&path, &grid, &palette).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_project(&path, &grid, &palette).unwrap();

        assert_eq!(swap_is_newer(&path), Some(false));
    }

    #[test]
    fn remove_swap_is_idempotent_on_missing() {
        let dir = tempdir();
        let path = dir.join("nothing_to_remove.maq");
        // File never existed — removal should not error.
        remove_swap(&path).unwrap();
    }

    #[test]
    fn remove_swap_deletes_sidecar() {
        let dir = tempdir();
        let path = dir.join("with_sidecar.maq");
        let (grid, palette) = (Grid::with_size(2, 2), Palette::default());
        write_swap(&path, &grid, &palette).unwrap();
        assert!(swap_path(&path).exists());

        remove_swap(&path).unwrap();
        assert!(!swap_path(&path).exists());
    }

    #[test]
    fn swap_is_readable_as_a_normal_project() {
        // The swap format equals the project format. This is what
        // lets the CLI inspect a swap without any new verb, and it's
        // what makes recovery a pure load call.
        let dir = tempdir();
        let path = dir.join("same_format.maq");
        let mut grid = Grid::with_size(4, 4);
        grid.paint(1, 1, 0, 2);
        let palette = Palette::default();

        write_swap(&path, &grid, &palette).unwrap();
        let (g2, _) = read_project(&swap_path(&path)).unwrap();
        assert_eq!(g2.cells, grid.cells);
    }
}
