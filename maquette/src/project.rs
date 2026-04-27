//! `.maq` project file format (pure / headless).
//!
//! Everything in this module is deliberately window-free so the
//! same load/save code can be driven by the GUI (through
//! `session::ProjectPlugin` in the binary) and by the CLI
//! (`maquette-cli`) and by tests. File dialogs and the Bevy-resource
//! wrapper (`CurrentProject`) live in the GUI binary; this module
//! just reads and writes bytes.
//!
//! ## File format · schema v4 (JSON)
//!
//! ```json
//! {
//!   "version": 4,
//!   "grid": { "w": 16, "h": 16, "cells": [ { "color_idx": 3, "height": 1 }, ... ] },
//!   "selected_color": 3,
//!   "palette": [ { "r": 0.9, "g": 0.3, "b": 0.35, "a": 1.0 }, null, ... ],
//!   "model_description": "minecraft-style grass dirt block",
//!   "texture_prefs": { "view_mode": "flat", "ignore_color_hint": false },
//!   "palette_meta": [
//!     { "override_hint": "patchy moss top",
//!       "texture": { "cache_key": "deadbeef…", "generated_at": 1700000000 } },
//!     null,
//!     {}
//!   ]
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
//! * `4` — adds three top-level fields for v0.10's AI texture
//!   pipeline:
//!   - `model_description: String` — single sentence the user types
//!     once that drives all per-slot prompts (D-1 GUI).
//!   - `texture_prefs: TexturePrefs` — `view_mode`
//!     (Flat / Textured) and `ignore_color_hint`.
//!   - `palette_meta: Vec<Option<PaletteSlotMeta>>` — parallel to
//!     `palette`, holds per-slot `override_hint` + `TextureHandle`.
//!     Length is realigned to match `palette` on load
//!     ([`Palette::ensure_meta_alignment`]) so a hand-edit that
//!     desyncs the two doesn't reject the file.
//!
//!   v3 (and earlier) files load unchanged — every new field is
//!   `#[serde(default)]`, and `model_description` defaults to the
//!   empty string, `texture_prefs` to its `Default`, and the meta
//!   vector to all-default. (v0.10 C)

use std::path::{Path, PathBuf};

use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::grid::{Cell, Grid, Palette};
use crate::texture_meta::{PaletteSlotMeta, TexturePrefs};

/// File extension used for Maquette project files.
pub const FILE_EXT: &str = "maq";

/// Suffix appended to a `.maq` to form its autosave sidecar. Chosen
/// over an in-place replacement (e.g. `foo.swap`) so that swap files
/// always sit next to their parent project, are visible to the user
/// in Finder / ls, and survive `find . -name '*.maq'` unless the user
/// opts into `*.maq.swap` too.
pub const SWAP_SUFFIX: &str = ".swap";

/// Schema version the exporter currently writes.
pub const SCHEMA_VERSION: u32 = 4;

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    version: u32,
    grid: GridPayload,
    selected_color: u8,
    /// Absent in v1 files; present from v2 onward. v3 allows `null`
    /// entries for deleted slots so palette indices stay stable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette: Option<Vec<Option<RgbaPayload>>>,
    /// v4: free-form description the user types once for the whole
    /// model. Drives D-1's "What is this model?" GUI affordance and
    /// becomes the seed for every per-slot prompt the worker sees.
    /// Pre-v4 files: missing → empty string.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    model_description: String,
    /// v4: per-project texture prefs. Pre-v4 files: missing →
    /// `TexturePrefs::default()` (Flat view + color-hint enabled).
    #[serde(default, skip_serializing_if = "is_default_texture_prefs")]
    texture_prefs: TexturePrefs,
    /// v4: per-slot meta parallel to `palette`. Length is *expected*
    /// to match the live `palette` length, but a desync (hand edit /
    /// truncated read) doesn't reject the file — the loader
    /// realigns via `Palette::ensure_meta_alignment` so the
    /// invariant is restored before the palette is observable. Pre-v4
    /// files: missing → all-default meta sized to match palette.
    ///
    /// `None` entries are accepted on read (treated as default meta)
    /// to keep the wire format symmetric with the sparse `palette`
    /// shape; on write we materialise every slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    palette_meta: Option<Vec<Option<PaletteSlotMeta>>>,
}

fn is_default_texture_prefs(p: &TexturePrefs) -> bool {
    *p == TexturePrefs::default()
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
    fn snapshot(grid: &Grid, palette: &Palette, meta: &ProjectMeta) -> Self {
        // Always emit `palette_meta` for live `.maq` files so that
        // the file we write in v0.10 C is forward-compatible: even
        // if the user never sets a hint or generates a texture,
        // future readers see "yes, this file is v4-aware".
        // Skip-serialising via `Option::is_none` would only
        // suppress it when palette is empty, which is a corner
        // case not worth special-casing.
        let palette_meta: Vec<Option<PaletteSlotMeta>> = palette
            .colors
            .iter()
            .zip(palette.slot_meta.iter())
            .map(|(color, m)| {
                // Encode "deleted slot with default meta" as
                // `null` to match the `palette` shape exactly —
                // makes hand-inspection of the JSON less noisy.
                if color.is_none() && m.is_empty() {
                    None
                } else {
                    Some(m.clone())
                }
            })
            .collect();

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
            model_description: meta.model_description.clone(),
            texture_prefs: meta.texture_prefs.clone(),
            palette_meta: Some(palette_meta),
        }
    }

    fn apply_to(
        self,
        grid: &mut Grid,
        palette: &mut Palette,
        meta: &mut ProjectMeta,
    ) -> Result<(), ProjectError> {
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

        // v4: per-slot meta. Pad / truncate to match the live
        // palette length unconditionally — a malformed file with
        // mismatched lengths must not crash the loader.
        if let Some(slots) = self.palette_meta {
            palette.slot_meta = slots
                .into_iter()
                .map(|m| m.unwrap_or_default())
                .collect();
        } else {
            // v3 (and earlier) files arrive here. Reset to all-default
            // so that loading a v3 file twice in a row produces
            // identical Palettes — important because EditHistory
            // hashes Palette state for "is the project dirty?" checks
            // (D-1 onward).
            palette.slot_meta = vec![Default::default(); palette.colors.len()];
        }
        palette.ensure_meta_alignment();

        meta.model_description = self.model_description;
        meta.texture_prefs = self.texture_prefs;

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

/// Project-level metadata introduced in schema v4.
///
/// Lives next to `Grid` + `Palette` rather than inside `Palette`
/// because both fields are project-wide (free-form description,
/// textures view mode, color-hint policy). The GUI keeps them in a
/// dedicated Bevy resource (`bin/main.rs` adds it as
/// `ResMut<ProjectMeta>` in v0.10 D-1); the headless lib treats it
/// as plain data.
///
/// Defaults are deliberately the "do nothing AI-related" pose so a
/// project that was never touched by the AI texture pipeline
/// round-trips with all-defaults and is byte-identical (modulo
/// field ordering) to itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectMeta {
    /// Free-form sentence the user types once for the whole model.
    /// Empty by default — older projects that never set it look
    /// the same as a brand-new v0.10 C project that hasn't been
    /// described yet.
    pub model_description: String,
    /// Project-scope texture preferences. See
    /// [`crate::texture_meta::TexturePrefs`].
    pub texture_prefs: TexturePrefs,
}

/// Read a `.maq` file from disk and return a fresh `(Grid, Palette)`.
///
/// **Drops project-level v4 meta** (`model_description`,
/// `texture_prefs`) — use [`read_project_with_meta`] when you need
/// those. This thin wrapper is preserved so the existing CLI
/// (`maquette-cli info`, `export`, `render`) doesn't need to learn
/// about meta yet. v3 and earlier files load identically through
/// either API.
pub fn read_project(path: &Path) -> Result<(Grid, Palette), ProjectError> {
    let (grid, palette, _meta) = read_project_with_meta(path)?;
    Ok((grid, palette))
}

/// v0.10 C: full read returning `(Grid, Palette, ProjectMeta)`.
///
/// Use this from the GUI (D-1 onward) so user-typed
/// `model_description` survives Open → Save → Open. Pre-v4 files
/// hand back a default `ProjectMeta`.
pub fn read_project_with_meta(
    path: &Path,
) -> Result<(Grid, Palette, ProjectMeta), ProjectError> {
    let text = std::fs::read_to_string(path)?;
    let pf: ProjectFile = serde_json::from_str(&text)?;
    let mut grid = Grid::with_size(pf.grid.w, pf.grid.h);
    let mut palette = Palette::default();
    let mut meta = ProjectMeta::default();
    pf.apply_to(&mut grid, &mut palette, &mut meta)?;
    Ok((grid, palette, meta))
}

/// Mutate an existing `Grid` and `Palette` to match the project at
/// `path`. Used by the GUI's `File → Open` path where it needs to
/// keep the existing `ResMut` handles stable.
///
/// **Drops project-level v4 meta** — same caveat as
/// [`read_project`]. The GUI uses
/// [`apply_to_grid_palette_meta`] in D-1 to also restore
/// `ResMut<ProjectMeta>`.
pub fn apply_to_grid_and_palette(
    path: &Path,
    grid: &mut Grid,
    palette: &mut Palette,
) -> Result<(), ProjectError> {
    let mut meta = ProjectMeta::default();
    apply_to_grid_palette_meta(path, grid, palette, &mut meta)
}

/// Full in-place load — restores `Grid`, `Palette`, and `ProjectMeta`
/// at once from `path`. Equivalent to [`read_project_with_meta`] but
/// reuses caller-owned mutable handles (matters for the GUI's Bevy
/// `ResMut` resources).
pub fn apply_to_grid_palette_meta(
    path: &Path,
    grid: &mut Grid,
    palette: &mut Palette,
    meta: &mut ProjectMeta,
) -> Result<(), ProjectError> {
    let text = std::fs::read_to_string(path)?;
    let pf: ProjectFile = serde_json::from_str(&text)?;
    pf.apply_to(grid, palette, meta)
}

/// Write `(Grid, Palette)` to `path` with default project meta.
///
/// **Caveat in v0.10 C:** if a project has a non-default
/// `ProjectMeta` (e.g. user typed a `model_description`), this
/// function will silently overwrite that meta with defaults on
/// save. The GUI must use [`write_project_with_meta`] from D-1
/// onward to avoid that. Existing CLI / autosave paths that don't
/// carry meta yet keep using this entry point and incur the
/// "writes blank description" trade-off until they're migrated —
/// for those code paths the file just stays unchanged on disk
/// (default → default), so it's a no-op in practice.
pub fn write_project(path: &Path, grid: &Grid, palette: &Palette) -> Result<(), ProjectError> {
    write_project_with_meta(path, grid, palette, &ProjectMeta::default())
}

/// v0.10 C: full write that preserves `ProjectMeta`.
pub fn write_project_with_meta(
    path: &Path,
    grid: &Grid,
    palette: &Palette,
    meta: &ProjectMeta,
) -> Result<(), ProjectError> {
    let pf = ProjectFile::snapshot(grid, palette, meta);
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

    // --- v0.10 C: schema v4 (model_description / texture_prefs / palette_meta) ---

    use crate::texture_meta::{PaletteSlotMeta, PaletteViewMode, TextureHandle, TexturePrefs};

    #[test]
    fn v3_file_loads_into_v4_build_with_default_meta() {
        // The headline #TEX-C verification requirement: a `.maq`
        // saved by a pre-v0.10 build must open in this build with
        // every new field at its default. Nothing about the file
        // touches `model_description` / `texture_prefs` /
        // `palette_meta`.
        let dir = tempdir();
        let path = dir.join("v3_round.maq");
        let v3 = serde_json::json!({
            "version": 3,
            "grid": { "w": 2, "h": 2, "cells": [
                { "color_idx": 0, "height": 1 },
                { "color_idx": null, "height": 0 },
                { "color_idx": null, "height": 0 },
                { "color_idx": null, "height": 0 },
            ]},
            "selected_color": 0,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                { "r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0 }
            ]
        });
        std::fs::write(&path, v3.to_string()).unwrap();

        let (g, p, meta) = read_project_with_meta(&path).unwrap();
        assert_eq!(g.cells[0].color_idx, Some(0));
        // Two live colors → two slot_meta entries, all default.
        assert_eq!(p.colors.len(), 2);
        assert_eq!(p.slot_meta.len(), 2);
        for m in &p.slot_meta {
            assert_eq!(m, &PaletteSlotMeta::default());
        }
        assert_eq!(meta, ProjectMeta::default());
    }

    #[test]
    fn v3_then_save_emits_v4_default_fields() {
        // From USER-TODO #TEX-C: "open a v3 .maq, hit Save, the
        // new file should have version=4 plus default new fields,
        // but loading it again must still produce the exact same
        // grid + palette + meta as before."
        let dir = tempdir();
        let v3_path = dir.join("v3_in.maq");
        let v3 = serde_json::json!({
            "version": 3,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": 0, "height": 1 } ] },
            "selected_color": 0,
            "palette": [ { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 } ]
        });
        std::fs::write(&v3_path, v3.to_string()).unwrap();
        let (g, p, _meta) = read_project_with_meta(&v3_path).unwrap();

        // Re-save through the legacy `write_project` API (the
        // Save path the GUI uses today).
        let v4_path = dir.join("v4_out.maq");
        write_project(&v4_path, &g, &p).unwrap();
        let raw = std::fs::read_to_string(&v4_path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], 4);
        // Defaults are skip-serialised (model_description = ""
        // and texture_prefs = default), so they should *not*
        // appear in the JSON — the file stays compact for users
        // who never touch the AI texture features. `palette_meta`
        // is always emitted so future readers see "this file is
        // v4-aware".
        assert!(parsed.get("model_description").is_none());
        assert!(parsed.get("texture_prefs").is_none());
        assert!(parsed["palette_meta"].is_array());

        // Round-trip equivalence.
        let (g2, p2, meta2) = read_project_with_meta(&v4_path).unwrap();
        assert_eq!(g2.cells, g.cells);
        assert_eq!(p2.colors, p.colors);
        assert_eq!(p2.slot_meta, p.slot_meta);
        assert_eq!(meta2, ProjectMeta::default());
    }

    #[test]
    fn v4_round_trip_preserves_model_description_and_prefs() {
        let dir = tempdir();
        let path = dir.join("with_meta.maq");
        let mut grid = Grid::with_size(4, 4);
        grid.paint(0, 0, 0, 1);
        let palette = Palette::default();
        let meta = ProjectMeta {
            model_description: "minecraft-style grass dirt".into(),
            texture_prefs: TexturePrefs {
                view_mode: PaletteViewMode::Textured,
                ignore_color_hint: true,
            },
        };

        write_project_with_meta(&path, &grid, &palette, &meta).unwrap();
        let (_g, _p, meta2) = read_project_with_meta(&path).unwrap();
        assert_eq!(meta2, meta);

        // Defaults are skip-serialised; non-defaults must appear.
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["model_description"], "minecraft-style grass dirt");
        assert_eq!(parsed["texture_prefs"]["view_mode"], "textured");
        assert_eq!(parsed["texture_prefs"]["ignore_color_hint"], true);
    }

    #[test]
    fn v4_round_trip_preserves_per_slot_override_hint_and_texture() {
        let dir = tempdir();
        let path = dir.join("per_slot.maq");
        let grid = Grid::with_size(4, 4);
        let mut palette = Palette::default();
        // Slot 0: override hint only.
        palette.set_override_hint(0, Some("rusty iron, scratched".into())).unwrap();
        // Slot 3 (default selected): full meta with texture handle.
        palette
            .set_override_hint(3, Some("patchy moss top".into()))
            .unwrap();
        palette
            .set_texture(
                3,
                Some(TextureHandle {
                    cache_key: "deadbeef".into(),
                    generated_at: 1_700_000_000,
                }),
            )
            .unwrap();

        write_project(&path, &grid, &palette).unwrap();
        let (_g, p2, _meta) = read_project_with_meta(&path).unwrap();
        assert_eq!(
            p2.meta(0).unwrap().override_hint.as_deref(),
            Some("rusty iron, scratched")
        );
        assert_eq!(
            p2.meta(3).unwrap().override_hint.as_deref(),
            Some("patchy moss top")
        );
        assert_eq!(
            p2.meta(3).unwrap().texture.as_ref().unwrap().cache_key,
            "deadbeef"
        );
        assert_eq!(
            p2.meta(3).unwrap().texture.as_ref().unwrap().generated_at,
            1_700_000_000
        );
        // Other slots stay default.
        assert!(p2.meta(1).unwrap().is_empty());
    }

    #[test]
    fn v4_palette_meta_length_mismatch_is_realigned_not_rejected() {
        // Hand-rolled file where `palette_meta` is shorter than
        // `palette`. A strict deserializer would refuse this; we
        // accept it and pad — the file came from somewhere
        // (older v0.10 C build, partial write) and rejecting it
        // outright would block recovery.
        let dir = tempdir();
        let path = dir.join("short_meta.maq");
        let payload = serde_json::json!({
            "version": 4,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": 0, "height": 1 } ] },
            "selected_color": 0,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                { "r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0 },
                { "r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0 },
            ],
            // Only one entry — palette has three. Realigner pads.
            "palette_meta": [
                { "override_hint": "carry-on hint" }
            ]
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        let (_g, p, _meta) = read_project_with_meta(&path).unwrap();
        assert_eq!(p.colors.len(), 3);
        assert_eq!(
            p.slot_meta.len(),
            3,
            "realigner must pad slot_meta to match colors"
        );
        assert_eq!(
            p.meta(0).unwrap().override_hint.as_deref(),
            Some("carry-on hint")
        );
        assert!(p.meta(1).unwrap().is_empty());
        assert!(p.meta(2).unwrap().is_empty());
    }

    #[test]
    fn v4_palette_meta_overflow_is_truncated() {
        let dir = tempdir();
        let path = dir.join("long_meta.maq");
        let payload = serde_json::json!({
            "version": 4,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": null, "height": 0 } ] },
            "selected_color": 0,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 }
            ],
            "palette_meta": [
                { "override_hint": "kept" },
                { "override_hint": "should be dropped" },
                { "override_hint": "should also be dropped" }
            ]
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        let (_g, p, _meta) = read_project_with_meta(&path).unwrap();
        assert_eq!(p.slot_meta.len(), 1);
        assert_eq!(p.meta(0).unwrap().override_hint.as_deref(), Some("kept"));
    }

    #[test]
    fn v4_palette_meta_handles_null_entries() {
        // The on-disk shape allows `null` for "deleted slot with
        // default meta" — keeps the JSON visually parallel to
        // the sparse `palette` array. We treat `null` the same
        // as an explicit empty `{}`.
        let dir = tempdir();
        let path = dir.join("null_meta.maq");
        let payload = serde_json::json!({
            "version": 4,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": 0, "height": 1 } ] },
            "selected_color": 0,
            "palette": [
                { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                null,
                { "r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0 }
            ],
            "palette_meta": [
                { "override_hint": "first" },
                null,
                {}
            ]
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        let (_g, p, _meta) = read_project_with_meta(&path).unwrap();
        assert_eq!(p.slot_meta.len(), 3);
        assert_eq!(
            p.meta(0).unwrap().override_hint.as_deref(),
            Some("first")
        );
        assert!(p.meta(1).unwrap().is_empty());
        assert!(p.meta(2).unwrap().is_empty());
    }

    #[test]
    fn legacy_write_project_emits_palette_meta_array() {
        // The legacy `write_project` API doesn't carry
        // ProjectMeta but still writes a v4-shaped file. This
        // matters because any consumer (autosave, CLI) that goes
        // through the legacy entry point must still produce
        // files that future v4 readers parse without complaint.
        let dir = tempdir();
        let path = dir.join("legacy_write.maq");
        let grid = Grid::with_size(4, 4);
        let palette = Palette::default();
        write_project(&path, &grid, &palette).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed["version"], 4);
        assert!(parsed["palette_meta"].is_array());
        let meta_array = parsed["palette_meta"].as_array().unwrap();
        assert_eq!(meta_array.len(), palette.colors.len());
        // Default palette has all live colors with empty meta;
        // serialiser emits them as `null` to keep the JSON lean
        // (see `snapshot()`).
        for entry in meta_array {
            assert!(
                entry.is_null() || entry.as_object().map(|o| o.is_empty()).unwrap_or(false),
                "default meta should serialize compactly; got {entry}"
            );
        }
    }

    #[test]
    fn future_version_5_is_still_rejected() {
        // Defensive: the `version > SCHEMA_VERSION` check still
        // works after the v4 bump. If a future build accidentally
        // writes version 5 ahead of us, we should refuse it
        // explicitly rather than silently mis-parse.
        let dir = tempdir();
        let path = dir.join("v5.maq");
        let payload = serde_json::json!({
            "version": 5,
            "grid": { "w": 1, "h": 1, "cells": [ { "color_idx": null, "height": 0 } ] },
            "selected_color": 0
        });
        std::fs::write(&path, payload.to_string()).unwrap();
        assert!(matches!(
            read_project(&path),
            Err(ProjectError::UnsupportedVersion(5))
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
