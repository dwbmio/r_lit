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

use bevy::prelude::Color;
use serde::{Deserialize, Serialize};

use crate::block_meta::BlockMeta;

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

    /// Bound block id (e.g. `"grass"`, `"oak_planks"`). Resolves
    /// through [`crate::block_meta::BlockMetaProvider::get`] —
    /// either the bundled `LocalProvider` or the cached hfrog
    /// catalog.
    ///
    /// When set, the GUI shows the block's name + thumbnail on the
    /// palette swatch, and the texgen prompt-derivation path uses
    /// the block's `texture_hint` (unless `override_hint` overrides
    /// it). Treat the empty string as "unbound" — the
    /// `set_block_id` setter normalises whitespace just like
    /// `override_hint` to avoid a fingertip-stray-space binding a
    /// nonexistent block.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
}

impl PaletteSlotMeta {
    pub fn is_empty(&self) -> bool {
        self.override_hint.is_none() && self.texture.is_none() && self.block_id.is_none()
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

// =====================================================================
// Prompt derivation
// =====================================================================

/// Resolve the final per-slot texgen prompt from project + slot +
/// (optional) bound block meta. Single source of truth — every
/// caller (CLI, GUI, batch tools) goes through this so
/// "what determines the prompt I send to Rustyme?" has one answer.
///
/// Priority, highest first:
///
/// 1. `slot.override_hint` — the user explicitly typed something
///    in the per-slot text field. Wins over everything else, even
///    when a block is bound. **This is the user's escape hatch.**
/// 2. Block's `texture_hint` (when `slot.block_id` is bound and the
///    `block` argument is `Some`). Combined with the project-wide
///    `model_description` for context: `"<model_description>, <hint>"`
///    when both exist; either alone otherwise.
/// 3. Block's `description` (fallback when its `texture_hint` is
///    empty — some local blocks are pure metadata without a
///    surface-rendering hint).
/// 4. `model_description + ", " + RGB color`. The bare path that
///    pre-block-meta projects took. RGB is suppressed when
///    `prefs.ignore_color_hint` is set (the A/B-test toggle the
///    user added in C-1 to compare "is the model wrong?" vs "is
///    the palette wrong?").
/// 5. As a last resort: `"colored block"` so the worker doesn't
///    receive an empty prompt (which `texgen-cpu`'s smart mode
///    handles, but `texgen-fal` would reject).
///
/// `slot_color` is the live RGB at the slot in question; passing
/// it explicitly (rather than holding a reference into `Palette`)
/// keeps this function callable from a Bevy system that already
/// holds `&Palette` borrowed elsewhere.
pub fn derive_texture_prompt(
    model_description: &str,
    slot: &PaletteSlotMeta,
    slot_color: Color,
    block: Option<&BlockMeta>,
    prefs: &TexturePrefs,
) -> String {
    // 1. Override always wins.
    if let Some(hint) = slot.override_hint.as_deref() {
        let trimmed = hint.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    let model_desc = model_description.trim();

    // 2 + 3. Block-bound path.
    if let Some(b) = block {
        let block_hint = if !b.texture_hint.trim().is_empty() {
            b.texture_hint.trim()
        } else if !b.description.trim().is_empty() {
            b.description.trim()
        } else {
            ""
        };
        let prompt = combine_with_model(model_desc, block_hint);
        if !prompt.is_empty() {
            return prompt;
        }
    }

    // 4. Pre-block-meta fallback.
    let mut parts = Vec::new();
    if !model_desc.is_empty() {
        parts.push(model_desc.to_string());
    }
    if !prefs.ignore_color_hint {
        parts.push(rgb_color_phrase(slot_color));
    }
    if !parts.is_empty() {
        return parts.join(", ");
    }

    // 5. Defensive default — never ship an empty prompt.
    "colored block".to_string()
}

fn combine_with_model(model_desc: &str, hint: &str) -> String {
    match (model_desc.is_empty(), hint.is_empty()) {
        (true, true) => String::new(),
        (false, true) => model_desc.to_string(),
        (true, false) => hint.to_string(),
        (false, false) => format!("{model_desc}, {hint}"),
    }
}

/// Convert a `Color` to a short phrase the texgen worker can use
/// as a colour hint. The output is intentionally simple ("rgb
/// (180, 80, 90)" / "deep red") rather than a CSS hex code, so the
/// LLM-flavoured `texgen-cpu smart` lane has something natural to
/// parse, while still being useful as a literal token in
/// `texgen-fal` flows.
fn rgb_color_phrase(c: Color) -> String {
    let s = c.to_srgba();
    let r = (s.red.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (s.green.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (s.blue.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("base color rgb({r}, {g}, {b})")
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
            ..Default::default()
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
            block_id: None,
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

    // ------------------------------------------------------------------
    // block_id field on PaletteSlotMeta (introduced alongside block_meta
    // module). Forward-compat tests live here because PaletteSlotMeta is
    // serialized inside the project file.
    // ------------------------------------------------------------------

    #[test]
    fn slot_meta_block_id_round_trips() {
        let m = PaletteSlotMeta {
            override_hint: None,
            texture: None,
            block_id: Some("grass".into()),
        };
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"block_id\":\"grass\""));
        let back: PaletteSlotMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn slot_meta_block_id_omitted_when_none() {
        // Same compactness contract as override_hint / texture: when
        // a slot has no binding the on-disk form omits the field
        // entirely so a 256-slot palette with all-defaults stays
        // tiny.
        let m = PaletteSlotMeta::default();
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn slot_meta_with_only_block_id_is_not_empty() {
        let m = PaletteSlotMeta {
            block_id: Some("oak_planks".into()),
            ..Default::default()
        };
        assert!(!m.is_empty());
    }

    #[test]
    fn slot_meta_old_payload_without_block_id_still_loads() {
        // v0.10 C-1 wrote payloads without the block_id field.
        // serde(default) catches that.
        let raw = r#"{"override_hint":"hello"}"#;
        let parsed: PaletteSlotMeta = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.override_hint.as_deref(), Some("hello"));
        assert!(parsed.block_id.is_none());
        assert!(parsed.texture.is_none());
    }

    // ------------------------------------------------------------------
    // derive_texture_prompt — priority chain unit tests.
    // ------------------------------------------------------------------

    fn slot_red() -> Color {
        Color::srgb(0.85, 0.30, 0.35) // close to default palette[0]
    }

    fn slot_with_override(s: &str) -> PaletteSlotMeta {
        PaletteSlotMeta {
            override_hint: Some(s.to_string()),
            ..Default::default()
        }
    }

    fn slot_with_block(id: &str) -> PaletteSlotMeta {
        PaletteSlotMeta {
            block_id: Some(id.to_string()),
            ..Default::default()
        }
    }

    fn block_grass_with_hint() -> BlockMeta {
        BlockMeta::new_local(
            "grass",
            "草地",
            "草地块描述",
            crate::grid::ShapeKind::Cube,
            crate::block_meta::RgbaColor::rgb(0.45, 0.80, 0.40),
            "patchy minecraft grass top",
        )
    }

    fn block_grass_no_hint() -> BlockMeta {
        // hint is empty — should fall back to description.
        BlockMeta::new_local(
            "grass",
            "草地",
            "草地块描述",
            crate::grid::ShapeKind::Cube,
            crate::block_meta::RgbaColor::rgb(0.45, 0.80, 0.40),
            "",
        )
    }

    #[test]
    fn override_hint_wins_over_everything() {
        let slot = slot_with_override("user-typed special");
        let prompt = derive_texture_prompt(
            "minecraft style",
            &slot,
            slot_red(),
            Some(&block_grass_with_hint()),
            &TexturePrefs::default(),
        );
        assert_eq!(prompt, "user-typed special");
    }

    #[test]
    fn block_hint_wins_when_no_override() {
        let slot = slot_with_block("grass");
        let prompt = derive_texture_prompt(
            "minecraft style grass dirt block",
            &slot,
            slot_red(),
            Some(&block_grass_with_hint()),
            &TexturePrefs::default(),
        );
        assert_eq!(
            prompt,
            "minecraft style grass dirt block, patchy minecraft grass top"
        );
    }

    #[test]
    fn block_falls_back_to_description_when_hint_empty() {
        let slot = slot_with_block("grass");
        let prompt = derive_texture_prompt(
            "voxel scene",
            &slot,
            slot_red(),
            Some(&block_grass_no_hint()),
            &TexturePrefs::default(),
        );
        assert_eq!(prompt, "voxel scene, 草地块描述");
    }

    #[test]
    fn pre_block_meta_path_falls_back_to_color_phrase() {
        // No override, no block bound (or unresolved).
        let slot = PaletteSlotMeta::default();
        let prompt = derive_texture_prompt(
            "voxel scene",
            &slot,
            slot_red(),
            None,
            &TexturePrefs::default(),
        );
        assert!(prompt.starts_with("voxel scene, base color rgb("));
    }

    #[test]
    fn ignore_color_hint_suppresses_color_phrase() {
        let slot = PaletteSlotMeta::default();
        let prompt = derive_texture_prompt(
            "voxel scene",
            &slot,
            slot_red(),
            None,
            &TexturePrefs {
                ignore_color_hint: true,
                ..Default::default()
            },
        );
        assert_eq!(prompt, "voxel scene");
    }

    #[test]
    fn empty_inputs_emit_safe_default() {
        // Defensive: never send an empty prompt to the worker. The
        // texgen-fal lane in particular would reject one outright;
        // even texgen-cpu's smart mode would just fall through to
        // a deterministic fallback.
        let slot = PaletteSlotMeta::default();
        let prompt = derive_texture_prompt(
            "",
            &slot,
            slot_red(),
            None,
            &TexturePrefs {
                ignore_color_hint: true,
                ..Default::default()
            },
        );
        assert_eq!(prompt, "colored block");
    }

    #[test]
    fn override_wins_even_when_whitespace_block_hint() {
        // Belt + suspenders: even if the LocalProvider author typoed
        // a hint with stray whitespace, override stays in charge.
        let slot = slot_with_override("forced hint");
        let mut block = block_grass_with_hint();
        block.texture_hint = "   ".into();
        let prompt = derive_texture_prompt(
            "anything",
            &slot,
            slot_red(),
            Some(&block),
            &TexturePrefs::default(),
        );
        assert_eq!(prompt, "forced hint");
    }
}
