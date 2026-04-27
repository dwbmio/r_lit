//! Portable palette serialization (`colors.json`).
//!
//! The `.maq` project file already stores the palette as a sparse
//! `Vec<Option<Color>>`, but a project file is a closed blob —
//! migrating a palette between projects means loading one, copying
//! colors, saving the other. This module exposes the palette as a
//! small, git-friendly JSON document so users can share palettes
//! without shipping a whole project.
//!
//! ## Format
//!
//! ```json
//! {
//!   "version": 1,
//!   "colors": ["#e64d59", null, "#4c85c1", "#d9b352", null, ...]
//! }
//! ```
//!
//! Each entry is either a 6-digit `#RRGGBB` sRGB color or `null` for
//! a deleted / never-populated slot. Slot **indices** are stable —
//! the `i`-th entry in `colors` corresponds exactly to palette slot
//! `i`, so importing a palette into an existing project does not
//! silently remap cells.
//!
//! Alpha is intentionally omitted: Maquette's palette model is
//! opaque-only, and any exporter-consumer that sees alpha would have
//! no way to represent it in the `Cell::color_idx` indirection.

use std::fs;
use std::path::Path;

use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::grid::{Palette, MAX_PALETTE_SLOTS};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(thiserror::Error, Debug)]
pub enum PaletteIoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported palette schema version {got}; expected {expected}")]
    UnsupportedVersion { got: u32, expected: u32 },
    #[error("palette too large: {got} slots, max {max}")]
    TooManySlots { got: usize, max: usize },
    #[error("invalid color at slot {index}: `{got}` — expected `#RRGGBB`")]
    BadColor { index: usize, got: String },
}

#[derive(Serialize, Deserialize)]
struct PaletteFile {
    version: u32,
    colors: Vec<Option<String>>,
}

/// Serialize a `Palette` to the portable JSON document. The result is
/// pretty-printed (2-space indent) so it diffs cleanly in git.
pub fn write_palette_json(palette: &Palette, out: &Path) -> Result<(), PaletteIoError> {
    let colors: Vec<Option<String>> = palette
        .colors
        .iter()
        .map(|slot| slot.map(color_to_hex))
        .collect();
    let doc = PaletteFile {
        version: SCHEMA_VERSION,
        colors,
    };
    let s = serde_json::to_string_pretty(&doc)?;
    fs::write(out, s)?;
    Ok(())
}

/// Load a palette JSON file into a fresh `Palette` with the selected
/// color defaulted to the first live slot (or 0 if the file has
/// none).
pub fn read_palette_json(path: &Path) -> Result<Palette, PaletteIoError> {
    let raw = fs::read_to_string(path)?;
    let doc: PaletteFile = serde_json::from_str(&raw)?;
    if doc.version != SCHEMA_VERSION {
        return Err(PaletteIoError::UnsupportedVersion {
            got: doc.version,
            expected: SCHEMA_VERSION,
        });
    }
    if doc.colors.len() > MAX_PALETTE_SLOTS {
        return Err(PaletteIoError::TooManySlots {
            got: doc.colors.len(),
            max: MAX_PALETTE_SLOTS,
        });
    }

    let mut colors = Vec::with_capacity(doc.colors.len());
    for (i, entry) in doc.colors.iter().enumerate() {
        match entry {
            None => colors.push(None),
            Some(s) => {
                let color = hex_to_color(s).ok_or_else(|| PaletteIoError::BadColor {
                    index: i,
                    got: s.clone(),
                })?;
                colors.push(Some(color));
            }
        }
    }

    // Pick the first live slot as the initial selection. An empty
    // palette round-trips to an empty palette with `selected = 0`;
    // the grid will be unreadable until the user adds a color, but
    // that's the user's problem, not ours.
    let selected = colors
        .iter()
        .position(|s| s.is_some())
        .map(|i| i as u8)
        .unwrap_or(0);

    Ok(Palette::from_colors(colors, selected))
}

/// Replace `palette.colors` with the contents of a palette JSON file
/// **in place**, preserving `palette.selected` where possible.
///
/// If `selected` no longer points to a live slot after the swap, it
/// snaps to the first live slot (or 0 if the imported palette is
/// entirely empty). Cells painted with colors whose indices exist in
/// both palettes keep working transparently; cells pointing at an
/// index that's now vacant will render as transparent until re-painted.
pub fn import_palette_into(palette: &mut Palette, path: &Path) -> Result<(), PaletteIoError> {
    let loaded = read_palette_json(path)?;
    palette.colors = loaded.colors;
    // colors length may have changed → restore the
    // `slot_meta.len() == colors.len()` invariant before we
    // hand the palette back. Imported color JSON has no
    // override_hint / texture concept, so any meta beyond the
    // imported length collapses to default (which is also what
    // the user would expect — those slots no longer exist).
    palette.ensure_meta_alignment();

    // Snap `selected` if it lost its slot.
    let selected = palette.selected as usize;
    let still_live = palette
        .colors
        .get(selected)
        .map(|s| s.is_some())
        .unwrap_or(false);
    if !still_live {
        let fallback = palette
            .colors
            .iter()
            .position(|s| s.is_some())
            .map(|i| i as u8)
            .unwrap_or(0);
        palette.selected = fallback;
    }
    Ok(())
}

fn color_to_hex(c: Color) -> String {
    let s = c.to_srgba();
    let r = (s.red.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let g = (s.green.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let b = (s.blue.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn hex_to_color(s: &str) -> Option<Color> {
    let raw = s.strip_prefix('#')?;
    if raw.len() != 6 {
        return None;
    }
    let bytes = u32::from_str_radix(raw, 16).ok()?;
    let r = ((bytes >> 16) & 0xff) as f32 / 255.0;
    let g = ((bytes >> 8) & 0xff) as f32 / 255.0;
    let b = (bytes & 0xff) as f32 / 255.0;
    Some(Color::srgb(r, g, b))
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid::{DeleteColorMode, Grid, Palette};

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1.5 / 255.0
    }

    #[test]
    fn round_trip_preserves_live_colors() {
        let palette = Palette::default();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("colors.json");
        write_palette_json(&palette, &path).unwrap();
        let loaded = read_palette_json(&path).unwrap();

        assert_eq!(loaded.colors.len(), palette.colors.len());
        for (orig, round) in palette.colors.iter().zip(loaded.colors.iter()) {
            match (orig, round) {
                (None, None) => {}
                (Some(a), Some(b)) => {
                    let sa = a.to_srgba();
                    let sb = b.to_srgba();
                    assert!(
                        close(sa.red, sb.red) && close(sa.green, sb.green) && close(sa.blue, sb.blue),
                        "color mismatch: {sa:?} vs {sb:?}"
                    );
                }
                _ => panic!("slot liveness mismatch"),
            }
        }
    }

    #[test]
    fn empty_slots_round_trip_as_null() {
        let mut palette = Palette::default();
        let mut grid = Grid::with_size(4, 4);
        palette.delete(1, &mut grid, DeleteColorMode::Erase);
        palette.delete(3, &mut grid, DeleteColorMode::Erase);

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("sparse.json");
        write_palette_json(&palette, &path).unwrap();

        // Peek at the raw JSON — the deleted slots should serialize as null.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("null"), "deleted slot should serialize as null: {raw}");

        let loaded = read_palette_json(&path).unwrap();
        assert!(loaded.colors[1].is_none());
        assert!(loaded.colors[3].is_none());
    }

    #[test]
    fn import_snaps_selection_when_old_slot_vanishes() {
        let mut palette = Palette {
            selected: 5,
            ..Palette::default()
        };

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("mini.json");
        // Write a palette with only two live slots, at indices 0 and 2.
        std::fs::write(
            &path,
            r##"{"version":1,"colors":["#ff0000",null,"#00ff00"]}"##,
        )
        .unwrap();

        import_palette_into(&mut palette, &path).unwrap();
        assert_eq!(palette.colors.len(), 3);
        assert!(palette.colors[0].is_some());
        assert!(palette.colors[1].is_none());
        assert!(palette.colors[2].is_some());
        // Slot 5 no longer exists; we snap to the first live slot.
        assert_eq!(palette.selected, 0);
    }

    #[test]
    fn import_preserves_selection_when_still_live() {
        let mut palette = Palette {
            selected: 2,
            ..Palette::default()
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("keep.json");
        std::fs::write(
            &path,
            r##"{"version":1,"colors":["#ff0000","#00ff00","#0000ff"]}"##,
        )
        .unwrap();
        import_palette_into(&mut palette, &path).unwrap();
        assert_eq!(palette.selected, 2, "selection still live → keep");
    }

    #[test]
    fn rejects_future_schema_version() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("future.json");
        std::fs::write(&path, r##"{"version":999,"colors":[]}"##).unwrap();
        let err = read_palette_json(&path).unwrap_err();
        assert!(matches!(err, PaletteIoError::UnsupportedVersion { .. }));
    }

    #[test]
    fn rejects_garbage_color_string() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("bad.json");
        std::fs::write(&path, r##"{"version":1,"colors":["not-a-color"]}"##).unwrap();
        let err = read_palette_json(&path).unwrap_err();
        assert!(matches!(err, PaletteIoError::BadColor { index: 0, .. }));
    }

    #[test]
    fn rejects_over_max_slots() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("huge.json");
        let colors: Vec<String> = (0..=MAX_PALETTE_SLOTS).map(|_| "\"#000000\"".into()).collect();
        let raw = format!(r##"{{"version":1,"colors":[{}]}}"##, colors.join(","));
        std::fs::write(&path, raw).unwrap();
        let err = read_palette_json(&path).unwrap_err();
        assert!(matches!(err, PaletteIoError::TooManySlots { .. }));
    }
}
