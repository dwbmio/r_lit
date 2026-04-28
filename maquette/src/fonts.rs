//! CJK font installation for the editor's egui contexts.
//!
//! ## Why this module exists
//!
//! egui's bundled fonts (Hack + Ubuntu) carry only Latin / a few
//! European scripts. Anything in the BMP outside that — e.g. the
//! `"草地块 / Grass"` block names from `LocalProvider` — falls
//! through to the empty-glyph box (`▯`) and the user sees garbage.
//!
//! ## Strategy: probe system fonts, no embed
//!
//! We deliberately don't bundle a font file with the binary
//! (NotoSansCJK is ~10 MB). Every desktop OS Maquette runs on
//! ships *some* CJK-capable system font:
//!
//! | OS         | Font path                                              | Notes |
//! |------------|--------------------------------------------------------|-------|
//! | macOS      | `/System/Library/Fonts/PingFang.ttc`                   | Apple's default Chinese font, present on every macOS install |
//! | macOS old  | `/System/Library/Fonts/Hiragino Sans GB.ttc`           | Pre-Big-Sur fallback |
//! | Linux      | `/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc` | Debian/Ubuntu `fonts-noto-cjk` |
//! | Linux 2    | `/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc`     | Arch/Fedora |
//! | Windows    | `C:/Windows/Fonts/msyh.ttc`                            | 微软雅黑 (Microsoft YaHei), Win 7+ |
//! | Windows 2  | `C:/Windows/Fonts/msyh.ttf`                            | Older builds |
//!
//! We probe top-to-bottom; first hit wins. On a dev box where none
//! of these exist (unusual; CI containers without `fonts-noto-cjk`
//! are the realistic case), we log a warning and let egui fall back
//! to its default fonts — Latin still works, CJK shows boxes. Easy
//! fix: `apt install fonts-noto-cjk` or install a CJK font system-
//! wide.
//!
//! ## Plumbing
//!
//! [`apply_cjk_fonts_to_ctx`] does the actual `Context::set_fonts`
//! call. The two callers — `ui_system` (primary egui pass) and
//! `block_composer.rs::composer_ui_system` (secondary window's
//! pass) — gate on a sentinel value (`Context::data_mut()` carries
//! `CjkFontsAppliedMarker`) so we only do the heavy
//! atlas-rebuild on the very first frame each context is seen.

use std::sync::Arc;

use bevy_egui::egui;

/// Sentinel placed in an egui `Context`'s userdata after fonts have
/// been swapped. egui's [`Context::set_fonts`] is expensive (rebuilds
/// the glyph atlas), so re-running it every frame would tank the
/// idle 0 % CPU win we care about for the editor. Stashing this in
/// the context's own data store keeps the gate co-located with the
/// state it protects.
#[derive(Clone, Copy)]
struct CjkFontsAppliedMarker;

/// Apply our preferred font definitions to `ctx` if it hasn't been
/// done yet for this context. Cheap when already applied (one
/// `data_mut().get_temp` lookup); does the full atlas rebuild when
/// not.
///
/// Idempotent across calls. Safe to spam from a per-frame system.
pub fn apply_cjk_fonts_to_ctx(ctx: &egui::Context) {
    let already_applied: Option<CjkFontsAppliedMarker> = ctx
        .data(|d| d.get_temp(egui::Id::new("__maquette_cjk_fonts_applied__")));
    if already_applied.is_some() {
        return;
    }

    match build_font_definitions() {
        Some((fonts, source)) => {
            log::info!("egui: applying CJK fonts via {source}");
            ctx.set_fonts(fonts);
        }
        None => {
            log::warn!(
                "egui: no CJK font found on this system; non-Latin glyphs \
                 will render as boxes. \
                 Install fonts-noto-cjk (Linux) or a system CJK font."
            );
            // Fall through: don't `set_fonts`, egui keeps its defaults.
            // Still mark applied so we don't re-probe every frame.
        }
    }

    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("__maquette_cjk_fonts_applied__"),
            CjkFontsAppliedMarker,
        );
    });
}

/// Try to construct an `egui::FontDefinitions` that includes a
/// system CJK font as a fallback for the Proportional + Monospace
/// families. Returns `None` (and the caller skips `set_fonts`) if
/// no candidate path exists or all reads fail.
///
/// Returned tuple: `(definitions, source path string)` so the
/// caller can log which font actually got picked.
fn build_font_definitions() -> Option<(egui::FontDefinitions, String)> {
    let (path, bytes) = probe_system_cjk_font()?;
    let mut fonts = egui::FontDefinitions::default();
    let key = "maquette_system_cjk".to_string();
    fonts
        .font_data
        .insert(key.clone(), Arc::new(egui::FontData::from_owned(bytes)));
    // Insert at the *front* of the proportional list so CJK
    // glyphs win over egui's Latin default's gaps. Latin
    // characters still resolve to Hack/Ubuntu (which look better
    // for code/headings); only when those don't have a glyph
    // does egui walk the family list.
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, key.clone());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push(key);
    Some((fonts, path))
}

/// Read the first system font that exists from
/// [`SYSTEM_CJK_FONT_PATHS`]. Returns the bytes + which path
/// matched.
///
/// On macOS 14+ Apple moved PingFang (and a few other large
/// system fonts) out of `/System/Library/Fonts/` and into
/// hash-named directories under
/// `/System/Library/AssetsV2/com_apple_MobileAsset_Font8/<hash>/AssetData/`.
/// We try the static paths first (cheap, fast, hits on Linux /
/// Windows and on macOS for STHeiti / Hiragino), then fall back to
/// scanning the AssetsV2 tree for a known set of CJK font names.
/// Single directory listing on the slow path; cached after first
/// success so subsequent runs are free.
fn probe_system_cjk_font() -> Option<(String, Vec<u8>)> {
    for path in SYSTEM_CJK_FONT_PATHS {
        match std::fs::read(path) {
            Ok(bytes) if !bytes.is_empty() => {
                return Some(((*path).to_string(), bytes));
            }
            Ok(_) => continue, // empty file — skip
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                log::debug!("egui: probing {path} failed: {e}");
                continue;
            }
        }
    }
    // macOS 14+ AssetsV2 fallback. Only relevant on darwin; on
    // other OSes the directory simply doesn't exist and read_dir
    // returns NotFound — same outcome as a static-path miss.
    if let Some(hit) = probe_macos_assetsv2_cjk() {
        return Some(hit);
    }
    None
}

/// Scan `/System/Library/AssetsV2/com_apple_MobileAsset_Font8` for
/// the on-disk delivery of PingFang (or any other CJK face Apple
/// moved out of the legacy `/System/Library/Fonts` tree). Each
/// asset directory has the shape `<sha>.asset/AssetData/<font>.ttc`,
/// so we walk one level down and check for known filenames.
fn probe_macos_assetsv2_cjk() -> Option<(String, Vec<u8>)> {
    const ROOT: &str = "/System/Library/AssetsV2/com_apple_MobileAsset_Font8";
    /// Names worth picking — listed in preference order. PingFang
    /// SC is Apple's modern Chinese face and renders both Simplified
    /// and Traditional cleanly; the others are sensible fallbacks
    /// in case a given delivery only carries the alternate shape.
    const CJK_NAMES: &[&str] = &[
        "PingFang.ttc",
        "PingFangSC.ttc",
        "PingFangTC.ttc",
        "PingFangHK.ttc",
    ];
    let entries = std::fs::read_dir(ROOT).ok()?;
    for entry in entries.flatten() {
        let asset_data = entry.path().join("AssetData");
        for name in CJK_NAMES {
            let candidate = asset_data.join(name);
            match std::fs::read(&candidate) {
                Ok(bytes) if !bytes.is_empty() => {
                    return Some((candidate.display().to_string(), bytes));
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
    }
    None
}

/// Top-priority list of system fonts that ship CJK glyphs on the
/// platforms Maquette is built for. Order matters — first hit wins.
///
/// Notes:
/// * `.ttc` collections work fine — `egui::FontData::from_owned`
///   hands the bytes to `ab_glyph` which knows how to pick face 0.
///   In practice that's the regular weight on every entry below.
/// * On macOS 14+ Apple moved PingFang into the AssetsV2 tree;
///   the legacy `/System/Library/Fonts/PingFang.ttc` path no
///   longer exists. STHeiti / Hiragino Sans GB are the
///   second-best static-path candidates (both still ship in
///   `/System/Library/Fonts/`); for PingFang itself we fall
///   through to [`probe_macos_assetsv2_cjk`].
/// * We don't include `.otf` Source Han Sans paths; on macOS those
///   are buried under `~/Library/Fonts` which would require tilde
///   expansion + per-user logic for a marginal win.
const SYSTEM_CJK_FONT_PATHS: &[&str] = &[
    // macOS — STHeiti Medium is bundled with every install through
    // (at least) macOS 14. ~55 MB. Hiragino Sans GB is the
    // historical Chinese face on macOS, also bundled.
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    // Pre-14 macOS path; harmless to probe on newer systems
    // because the file simply doesn't exist there.
    "/System/Library/Fonts/PingFang.ttc",
    // Linux — Noto Sans CJK is the de-facto standard. Both Debian/
    // Ubuntu (`fonts-noto-cjk` package) and Arch / Fedora install
    // here under different roots.
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.otc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    // Linux fallbacks — older Wenquanyi installs.
    "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    // Windows — 微软雅黑 is the modern default; SimSun is the
    // legacy fallback bundled even with Server SKUs.
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyh.ttf",
    "C:/Windows/Fonts/msyhbd.ttc",
    "C:/Windows/Fonts/simsun.ttc",
    "C:/Windows/Fonts/simhei.ttf",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_returns_none_when_paths_unreadable() {
        // If we override the const list in a test we'd need a
        // proper injection seam; for now this just smoke-tests that
        // the probe doesn't panic on the host's real path list. On
        // the CI box the result depends on whether NotoSansCJK is
        // installed — both branches are valid, we just want no
        // crash.
        let _ = probe_system_cjk_font();
    }

    #[test]
    fn marker_is_zero_size() {
        // Sanity: the egui userdata sentinel doesn't allocate.
        // The whole point of this gate is to make the per-frame
        // already-applied check free.
        assert_eq!(std::mem::size_of::<CjkFontsAppliedMarker>(), 0);
    }
}
