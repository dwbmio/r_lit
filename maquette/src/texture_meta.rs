//! Per-palette-slot texture metadata + project-wide texture prefs.
//!
//! Introduced in **schema v4** (v0.10 C). Lives in the headless lib
//! because the load/save path, the CLI, and the GUI all need to be
//! able to inspect or mutate these without dragging Bevy into the
//! picture. The actual PNG bytes never live here — they sit in
//! `~/.cache/maquette/textures/<cache_key>.png`, owned by
//! [`crate::texgen`]. This module just holds the *handle*.
//!
//! ## Field stability
//!
//! Every struct here is wire-stable: `serde(default)` on every field,
//! and additions go to the *end* of the struct. Reorder a field at
//! your peril — older builds need to be able to skip past unknown
//! ones without breaking the whole project file.
//!
//! ## What lives here vs. in `grid.rs`
//!
//! `grid::Palette` carries a `Vec<PaletteSlotMeta>` parallel to its
//! existing `Vec<Option<Color>>`. The two stay length-synchronised
//! through `Palette::add` / `delete` / `apply_meta` — see those for
//! the invariant. We chose the parallel-vec layout (over a unified
//! `PaletteSlot { color, hint, texture }`) explicitly to minimise the
//! blast radius of v0.10 C across the existing call sites that read
//! `palette.colors`.

use serde::{Deserialize, Serialize};

/// How a generated texture is referenced from a `.maq` file.
///
/// We keep this *small on purpose*. The PNG bytes live on disk,
/// addressed by `cache_key` (a 64-char SHA-256 hex string from
/// [`crate::texgen::TextureRequest::cache_key`]). `generated_at`
/// is a unix epoch in seconds — chosen over RFC3339 strings because
/// integer compare is cheap, the value is only ever read by the
/// "stale texture" sweeper, and it survives timezone weirdness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TextureHandle {
    /// SHA-256 hex of the originating `TextureRequest`. Same value
    /// the on-disk cache file is named after (`<cache_key>.png`).
    /// Resolved through [`crate::texgen::cache_get`] when the GUI
    /// wants to upload the texture to a wgpu device.
    pub cache_key: String,
    /// When the texture was generated, in unix epoch *seconds*.
    /// Used by future cache-pruning logic ("evict textures older
    /// than 30 days that no live `.maq` references"). Defaults to
    /// `0` for hand-edited / fixture data — pruning sweep treats
    /// `0` as "ageless, do not evict".
    #[serde(default)]
    pub generated_at: i64,
}

/// Per-slot metadata that travels alongside the palette color.
///
/// Kept separate from `Color` so that everything that already
/// pattern-matched on `palette.colors[i]` keeps working unchanged.
/// New code that wants the meta calls
/// [`crate::grid::Palette::meta`] / [`crate::grid::Palette::meta_mut`]
/// instead.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteSlotMeta {
    /// Per-slot prompt override. When `Some`, the GUI uses this verbatim
    /// for that slot's `texture.gen` task instead of deriving one
    /// from the model-wide prompt + the slot's color.
    ///
    /// Empty strings are normalized to `None` on save so we never
    /// disagree with "no override" semantically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub override_hint: Option<String>,

    /// Last successful texture generation for this slot. `None`
    /// means either the slot has never been textured or the
    /// generation failed; the GUI falls back to the slot color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub texture: Option<TextureHandle>,
}

impl PaletteSlotMeta {
    pub fn is_empty(&self) -> bool {
        self.override_hint.is_none() && self.texture.is_none()
    }
}

/// How the GUI is currently rendering palette colors. Persisted on
/// the project file so that re-opening a textured project keeps
/// showing the textures (instead of always defaulting to flat).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteViewMode {
    /// Solid color per slot — the pre-v0.10 behaviour. Default.
    #[default]
    Flat,
    /// Sample from each slot's `texture`. Falls through to `Flat`
    /// for slots whose `PaletteSlotMeta::texture` is `None`.
    Textured,
}

/// Project-wide texture preferences.
///
/// Distinct from [`PaletteSlotMeta`] in that these belong to the
/// **project**, not to any slot — they survive palette deletions,
/// re-orderings, etc.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TexturePrefs {
    /// Flat / Textured view toggle (defaults to Flat).
    #[serde(default)]
    pub view_mode: PaletteViewMode,
    /// When `true`, the GUI does *not* inject the slot's RGB color
    /// as a hint into the worker prompt. The user might want this
    /// to compare "is the model right?" against "is the palette
    /// right?". Defaults to `false` — color hint enabled.
    #[serde(default)]
    pub ignore_color_hint: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texture_handle_round_trips() {
        let h = TextureHandle {
            cache_key: "abc123".into(),
            generated_at: 1_700_000_000,
        };
        let json = serde_json::to_string(&h).unwrap();
        let back: TextureHandle = serde_json::from_str(&json).unwrap();
        assert_eq!(back, h);
    }

    #[test]
    fn texture_handle_old_files_without_generated_at_still_load() {
        // We added `generated_at` after `cache_key`. Older test
        // fixtures or hand-rolled files might not carry it; serde
        // default kicks it to 0.
        let parsed: TextureHandle = serde_json::from_str(r#"{"cache_key":"x"}"#).unwrap();
        assert_eq!(parsed.generated_at, 0);
        assert_eq!(parsed.cache_key, "x");
    }

    #[test]
    fn slot_meta_default_is_empty() {
        let m = PaletteSlotMeta::default();
        assert!(m.is_empty());
        // Empty meta serialises to `{}` (both fields skipped). That
        // way the on-disk payload doesn't bloat by 1.5 KB on a
        // 256-slot palette where nobody has set anything yet.
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn slot_meta_round_trips_with_override_only() {
        let m = PaletteSlotMeta {
            override_hint: Some("rusty iron, scratched".into()),
            texture: None,
        };
        let s = serde_json::to_string(&m).unwrap();
        assert!(!s.contains("texture"), "missing texture should be skipped");
        let back: PaletteSlotMeta = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn slot_meta_round_trips_with_full_payload() {
        let m = PaletteSlotMeta {
            override_hint: Some("mossy stone".into()),
            texture: Some(TextureHandle {
                cache_key: "deadbeef".into(),
                generated_at: 1_700_000_000,
            }),
        };
        let json = serde_json::to_string(&m).unwrap();
        let back: PaletteSlotMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn texture_prefs_default_is_flat_with_color_hint() {
        let prefs = TexturePrefs::default();
        assert_eq!(prefs.view_mode, PaletteViewMode::Flat);
        assert!(!prefs.ignore_color_hint);
    }

    #[test]
    fn texture_prefs_round_trips() {
        let prefs = TexturePrefs {
            view_mode: PaletteViewMode::Textured,
            ignore_color_hint: true,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: TexturePrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, prefs);
    }

    #[test]
    fn view_mode_serialises_as_snake_case() {
        let json = serde_json::to_string(&PaletteViewMode::Textured).unwrap();
        assert_eq!(json, "\"textured\"");
        let back: PaletteViewMode = serde_json::from_str("\"flat\"").unwrap();
        assert_eq!(back, PaletteViewMode::Flat);
    }
}
