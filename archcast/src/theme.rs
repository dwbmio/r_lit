//! Structured global visual theme system (Design Tokens) with built-in defaults
//! and external YAML/JSON CRUD registry (`~/.config/archcast/themes` or `ARCHCAST_THEMES_DIR`).
//!
//! Controls HTML chrome, SVG canvas/card/edge colors, semantic `Tone` mapping,
//! WebGPU / GIF pulse palettes, and synced Mermaid `themeVariables` from one schema.

use crate::error::{ArchcastError, Result};
use crate::schema::Tone;
use crate::write_atomic;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const THEME_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_THEME_ID: &str = "starlink-light";
pub const BUILTIN_THEME_IDS: [&str; 5] = [
    "starlink-light",
    "obsidian-neon",
    "paper-editorial",
    "blueprint-ink",
    "catppuccin-mocha",
];

/// Full structured visual theme configuration (serializable to YAML/JSON).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    #[serde(default = "default_theme_schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    pub color_scheme: String,
    pub is_dark: bool,
    pub surface: SurfaceTokens,
    pub ink: InkTokens,
    pub chrome: ChromeTokens,
    pub table: TableTokens,
    pub warning: WarningTokens,
    pub tones: ToneTokens,
    pub motion: MotionTokens,
}

fn default_theme_schema_version() -> u32 {
    THEME_SCHEMA_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceTokens {
    pub page_bg: String,
    pub card_bg: String,
    pub card_border: String,
    pub card_shadow: String,
    pub svg_bg: String,
    pub node_bg: String,
    pub node_border: String,
    pub subcard_bg: String,
    pub subcard_border: String,
    pub callout_bg: String,
    pub callout_border: String,
    pub code_bg: String,
    pub codebox_bg: String,
    pub codebox_border: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InkTokens {
    pub page_fg: String,
    pub lead_fg: String,
    pub note_fg: String,
    pub code_fg: String,
    pub svg_title: String,
    pub svg_subtitle: String,
    pub svg_meta: String,
    pub col_header: String,
    pub node_title: String,
    pub node_text: String,
    pub node_mono: String,
    pub node_footer: String,
    pub subcard_title: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChromeTokens {
    pub toc_link: String,
    pub h2_accent: String,
    pub btn_bg: String,
    pub btn_fg: String,
    pub btn_border: String,
    pub btn_hover_bg: String,
    pub pill_bg: String,
    pub pill_fg: String,
    pub edge_spine: String,
    pub edge_stroke: String,
    pub edge_label_bg: String,
    pub edge_label_fg: String,
    pub badge_opacity: String,
    pub ver_bg_alpha: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableTokens {
    pub head_bg: String,
    pub head_fg: String,
    pub border: String,
    pub row_border: String,
    pub hover_bg: String,
    pub grp_bg: String,
    pub grp_fg: String,
    pub cnt_fg: String,
    pub small_fg: String,
    pub down_fg: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarningTokens {
    pub hint_bg: String,
    pub hint_fg: String,
    pub hint_accent: String,
    pub banner_bg: String,
    pub banner_border: String,
    pub banner_accent: String,
    pub banner_title: String,
    pub banner_body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToneTokens {
    pub default: String,
    pub muted: String,
    pub blue: String,
    pub orange: String,
    pub green: String,
    pub emerald: String,
    pub amber: String,
    pub purple: String,
    pub gold: String,
    pub red: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MotionTokens {
    pub pulse_hex: [String; 4],
    pub beacon_red: String,
    pub beacon_green: String,
}

/// Summary record returned by `archcast theme list`.
#[derive(Debug, Clone, Serialize)]
pub struct ThemeSummary {
    pub id: String,
    pub display_name: String,
    pub color_scheme: String,
    pub source: String,
    pub overrides_builtin: bool,
    pub path: Option<PathBuf>,
}

impl ThemeConfig {
    pub fn tone_hex(&self, tone: Tone) -> &str {
        match tone {
            Tone::Default => &self.tones.default,
            Tone::Muted => &self.tones.muted,
            Tone::Blue => &self.tones.blue,
            Tone::Orange => &self.tones.orange,
            Tone::Green => &self.tones.green,
            Tone::Emerald => &self.tones.emerald,
            Tone::Amber => &self.tones.amber,
            Tone::Purple => &self.tones.purple,
            Tone::Gold => &self.tones.gold,
            Tone::Red => &self.tones.red,
        }
    }

    pub fn svg_bg_rgba(&self) -> [u8; 4] {
        parse_hex_rgba(&self.surface.svg_bg).unwrap_or([255, 255, 255, 255])
    }

    pub fn pulse_rgb(&self) -> [[f32; 3]; 4] {
        [
            parse_hex_rgb(&self.motion.pulse_hex[0]).unwrap_or([15.0, 138.0, 95.0]),
            parse_hex_rgb(&self.motion.pulse_hex[1]).unwrap_or([75.0, 131.0, 212.0]),
            parse_hex_rgb(&self.motion.pulse_hex[2]).unwrap_or([224.0, 139.0, 60.0]),
            parse_hex_rgb(&self.motion.pulse_hex[3]).unwrap_or([123.0, 97.0, 201.0]),
        ]
    }

    pub fn beacon_red_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.motion.beacon_red).unwrap_or([217.0, 83.0, 79.0])
    }

    pub fn beacon_green_rgb(&self) -> [f32; 3] {
        parse_hex_rgb(&self.motion.beacon_green).unwrap_or([15.0, 138.0, 95.0])
    }

    /// Generate a Mermaid `%%{init: ...}%%` directive matching this theme's palette.
    pub fn mermaid_init_directive(&self) -> String {
        format!(
            "%%{{init: {{'theme': 'base', 'themeVariables': {{'background': '{}', 'primaryColor': '{}', 'primaryTextColor': '{}', 'primaryBorderColor': '{}', 'lineColor': '{}', 'secondaryColor': '{}', 'tertiaryColor': '{}', 'clusterBkg': '{}', 'clusterBorder': '{}'}}}}}}%%",
            self.surface.svg_bg,
            self.surface.node_bg,
            self.ink.node_title,
            self.surface.node_border,
            self.chrome.edge_stroke,
            self.surface.subcard_bg,
            self.surface.callout_bg,
            self.surface.codebox_bg,
            self.surface.card_border,
        )
    }

    /// Apply a single dot-path field override (e.g. `surface.page_bg=#101018`, `tones.red=#ff4d4f`).
    pub fn apply_field(&mut self, key: &str, value: &str) -> Result<()> {
        let v = value.to_string();
        match key.trim() {
            "display_name" => self.display_name = v,
            "color_scheme" => {
                if value != "light" && value != "dark" {
                    return Err(ArchcastError::Usage(format!(
                        "color_scheme must be 'light' or 'dark', got '{value}'"
                    )));
                }
                self.is_dark = value == "dark";
                self.color_scheme = v;
            }
            "is_dark" => {
                let parsed = value.parse::<bool>().map_err(|_| {
                    ArchcastError::Usage(format!("is_dark must be true/false, got '{value}'"))
                })?;
                self.is_dark = parsed;
                self.color_scheme = if parsed { "dark" } else { "light" }.to_string();
            }

            // surface.*
            "surface.page_bg" => self.surface.page_bg = v,
            "surface.card_bg" => self.surface.card_bg = v,
            "surface.card_border" => self.surface.card_border = v,
            "surface.card_shadow" => self.surface.card_shadow = v,
            "surface.svg_bg" => self.surface.svg_bg = v,
            "surface.node_bg" => self.surface.node_bg = v,
            "surface.node_border" => self.surface.node_border = v,
            "surface.subcard_bg" => self.surface.subcard_bg = v,
            "surface.subcard_border" => self.surface.subcard_border = v,
            "surface.callout_bg" => self.surface.callout_bg = v,
            "surface.callout_border" => self.surface.callout_border = v,
            "surface.code_bg" => self.surface.code_bg = v,
            "surface.codebox_bg" => self.surface.codebox_bg = v,
            "surface.codebox_border" => self.surface.codebox_border = v,

            // ink.*
            "ink.page_fg" => self.ink.page_fg = v,
            "ink.lead_fg" => self.ink.lead_fg = v,
            "ink.note_fg" => self.ink.note_fg = v,
            "ink.code_fg" => self.ink.code_fg = v,
            "ink.svg_title" => self.ink.svg_title = v,
            "ink.svg_subtitle" => self.ink.svg_subtitle = v,
            "ink.svg_meta" => self.ink.svg_meta = v,
            "ink.col_header" => self.ink.col_header = v,
            "ink.node_title" => self.ink.node_title = v,
            "ink.node_text" => self.ink.node_text = v,
            "ink.node_mono" => self.ink.node_mono = v,
            "ink.node_footer" => self.ink.node_footer = v,
            "ink.subcard_title" => self.ink.subcard_title = v,

            // chrome.*
            "chrome.toc_link" => self.chrome.toc_link = v,
            "chrome.h2_accent" => self.chrome.h2_accent = v,
            "chrome.btn_bg" => self.chrome.btn_bg = v,
            "chrome.btn_fg" => self.chrome.btn_fg = v,
            "chrome.btn_border" => self.chrome.btn_border = v,
            "chrome.btn_hover_bg" => self.chrome.btn_hover_bg = v,
            "chrome.pill_bg" => self.chrome.pill_bg = v,
            "chrome.pill_fg" => self.chrome.pill_fg = v,
            "chrome.edge_spine" => self.chrome.edge_spine = v,
            "chrome.edge_stroke" => self.chrome.edge_stroke = v,
            "chrome.edge_label_bg" => self.chrome.edge_label_bg = v,
            "chrome.edge_label_fg" => self.chrome.edge_label_fg = v,
            "chrome.badge_opacity" => self.chrome.badge_opacity = v,
            "chrome.ver_bg_alpha" => self.chrome.ver_bg_alpha = v,

            // table.*
            "table.head_bg" => self.table.head_bg = v,
            "table.head_fg" => self.table.head_fg = v,
            "table.border" => self.table.border = v,
            "table.row_border" => self.table.row_border = v,
            "table.hover_bg" => self.table.hover_bg = v,
            "table.grp_bg" => self.table.grp_bg = v,
            "table.grp_fg" => self.table.grp_fg = v,
            "table.cnt_fg" => self.table.cnt_fg = v,
            "table.small_fg" => self.table.small_fg = v,
            "table.down_fg" => self.table.down_fg = v,

            // warning.*
            "warning.hint_bg" => self.warning.hint_bg = v,
            "warning.hint_fg" => self.warning.hint_fg = v,
            "warning.hint_accent" => self.warning.hint_accent = v,
            "warning.banner_bg" => self.warning.banner_bg = v,
            "warning.banner_border" => self.warning.banner_border = v,
            "warning.banner_accent" => self.warning.banner_accent = v,
            "warning.banner_title" => self.warning.banner_title = v,
            "warning.banner_body" => self.warning.banner_body = v,

            // tones.*
            "tones.default" => self.tones.default = v,
            "tones.muted" => self.tones.muted = v,
            "tones.blue" => self.tones.blue = v,
            "tones.orange" => self.tones.orange = v,
            "tones.green" => self.tones.green = v,
            "tones.emerald" => self.tones.emerald = v,
            "tones.amber" => self.tones.amber = v,
            "tones.purple" => self.tones.purple = v,
            "tones.gold" => self.tones.gold = v,
            "tones.red" => self.tones.red = v,

            // motion.*
            "motion.beacon_red" => self.motion.beacon_red = v,
            "motion.beacon_green" => self.motion.beacon_green = v,
            "motion.pulse_hex" => {
                let parts: Vec<String> = value
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if parts.len() != 4 {
                    return Err(ArchcastError::Usage(
                        "motion.pulse_hex requires 4 comma-separated hex colors (e.g. #0f8a5f,#4b83d4,#e08b3c,#7b61c9)".to_string(),
                    ));
                }
                self.motion.pulse_hex = [
                    parts[0].clone(),
                    parts[1].clone(),
                    parts[2].clone(),
                    parts[3].clone(),
                ];
            }
            other => {
                return Err(ArchcastError::Usage(format!(
                    "unknown theme token path '{other}' (use e.g. surface.page_bg, ink.page_fg, chrome.h2_accent, tones.emerald)"
                )));
            }
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        validate_theme_id(&self.id)?;
        if self.display_name.trim().is_empty() {
            return Err(ArchcastError::Usage(
                "theme display_name must not be empty".to_string(),
            ));
        }
        if self.color_scheme != "light" && self.color_scheme != "dark" {
            return Err(ArchcastError::Usage(format!(
                "theme '{}' color_scheme must be 'light' or 'dark', got '{}'",
                self.id, self.color_scheme
            )));
        }
        if parse_hex_rgba(&self.surface.svg_bg).is_none() {
            return Err(ArchcastError::Usage(format!(
                "theme '{}' surface.svg_bg must be a valid #RRGGBB hex color, got '{}'",
                self.id, self.surface.svg_bg
            )));
        }
        for (idx, h) in self.motion.pulse_hex.iter().enumerate() {
            if parse_hex_rgb(h).is_none() {
                return Err(ArchcastError::Usage(format!(
                    "theme '{}' motion.pulse_hex[{idx}] must be a valid #RRGGBB hex color, got '{h}'",
                    self.id
                )));
            }
        }
        if parse_hex_rgb(&self.motion.beacon_red).is_none()
            || parse_hex_rgb(&self.motion.beacon_green).is_none()
        {
            return Err(ArchcastError::Usage(format!(
                "theme '{}' motion beacon colors must be valid #RRGGBB hex colors",
                self.id
            )));
        }
        Ok(())
    }
}

/// Validate a safe single-segment theme identifier (no path separators or traversal).
pub fn validate_theme_id(id: &str) -> Result<()> {
    let trimmed = id.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return Err(ArchcastError::Usage(format!(
            "theme id '{id}' must be 1..=64 characters"
        )));
    }
    let valid = trimmed.bytes().enumerate().all(|(i, b)| {
        if i == 0 {
            b.is_ascii_alphanumeric()
        } else {
            b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
        }
    });
    if !valid {
        return Err(ArchcastError::Usage(format!(
            "theme id '{id}' must start with ASCII alphanumeric and contain only [a-zA-Z0-9_-]"
        )));
    }
    Ok(())
}

/// Resolve the external themes directory: explicit `--themes-dir`, else `ARCHCAST_THEMES_DIR`,
/// else `<home>/.config/archcast/themes`.
pub fn resolve_themes_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(p) = explicit {
        return p.to_path_buf();
    }
    if let Ok(env_dir) = std::env::var("ARCHCAST_THEMES_DIR")
        && !env_dir.trim().is_empty()
    {
        return PathBuf::from(env_dir);
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        return PathBuf::from(xdg).join("archcast").join("themes");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.trim().is_empty()
    {
        return PathBuf::from(home)
            .join(".config")
            .join("archcast")
            .join("themes");
    }
    if let Ok(appdata) = std::env::var("APPDATA")
        && !appdata.trim().is_empty()
    {
        return PathBuf::from(appdata).join("archcast").join("themes");
    }
    PathBuf::from(".archcast-themes")
}

/// Return a bundled built-in theme by ID, if it exists.
pub fn builtin_theme(id: &str) -> Option<ThemeConfig> {
    match id.trim().to_ascii_lowercase().as_str() {
        "starlink-light" | "starlink_light" | "default" => Some(starlink_light()),
        "obsidian-neon" | "obsidian_neon" => Some(obsidian_neon()),
        "paper-editorial" | "paper_editorial" => Some(paper_editorial()),
        "blueprint-ink" | "blueprint_ink" => Some(blueprint_ink()),
        "catppuccin-mocha" | "catppuccin_mocha" => Some(catppuccin_mocha()),
        _ => None,
    }
}

pub fn default_theme() -> ThemeConfig {
    starlink_light()
}

/// Load a theme YAML/JSON file with `extends` inheritance resolution (supports partial overrides!).
pub fn load_theme_file(path: &Path, themes_dir: Option<&Path>) -> Result<ThemeConfig> {
    let raw = std::fs::read_to_string(path).map_err(|source| ArchcastError::ReadInput {
        path: path.to_path_buf(),
        source,
    })?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let patch_val: serde_json::Value = if ext == "json" {
        serde_json::from_str(&raw).map_err(|source| ArchcastError::JsonParse {
            path: path.to_path_buf(),
            source,
        })?
    } else {
        serde_yaml_ng::from_str(&raw).map_err(|source| ArchcastError::YamlParse {
            path: path.to_path_buf(),
            source,
        })?
    };

    merge_theme_value(patch_val, themes_dir, 0)
}

fn merge_theme_value(
    patch_val: serde_json::Value,
    themes_dir: Option<&Path>,
    depth: usize,
) -> Result<ThemeConfig> {
    if depth > 4 {
        return Err(ArchcastError::Usage(
            "theme `extends` inheritance chain exceeds max depth (4)".to_string(),
        ));
    }
    let self_id = patch_val
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let extends_id = patch_val
        .get("extends")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let base = match extends_id {
        Some(ref parent) if self_id.as_deref() == Some(parent.as_str()) => {
            builtin_theme(parent).ok_or_else(|| {
                ArchcastError::Usage(format!(
                    "theme '{parent}' cannot extend itself because no bundled built-in theme has that id"
                ))
            })?
        }
        Some(ref parent) => resolve_theme_inner(parent, themes_dir, depth + 1)?,
        None => default_theme(),
    };

    let mut merged = serde_json::to_value(&base)
        .map_err(|e| ArchcastError::Usage(format!("failed to encode base theme: {e}")))?;
    deep_merge_json(&mut merged, patch_val);

    let cfg: ThemeConfig = serde_json::from_value(merged)
        .map_err(|e| ArchcastError::Usage(format!("invalid theme config structure: {e}")))?;
    cfg.validate()?;
    Ok(cfg)
}

fn deep_merge_json(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(bmap), serde_json::Value::Object(pmap)) => {
            for (k, v) in pmap {
                if v.is_null() {
                    continue;
                }
                match bmap.get_mut(&k) {
                    Some(existing) => deep_merge_json(existing, v),
                    None => {
                        bmap.insert(k, v);
                    }
                }
            }
        }
        (slot, val) => *slot = val,
    }
}

/// Resolve a theme reference:
/// 1. Existing file path (`.yaml`/`.yml`/`.json`)
/// 2. External theme in `<themes_dir>/<ref>.yaml` (or `.yml`/`.json`)
/// 3. Bundled built-in theme preset
pub fn resolve_theme(theme_ref: Option<&str>, themes_dir: Option<&Path>) -> Result<ThemeConfig> {
    let target = theme_ref
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_THEME_ID);
    resolve_theme_inner(target, themes_dir, 0)
}

fn resolve_theme_inner(
    theme_ref: &str,
    themes_dir: Option<&Path>,
    depth: usize,
) -> Result<ThemeConfig> {
    let direct_path = Path::new(theme_ref);
    if direct_path.is_file() {
        let raw = std::fs::read_to_string(direct_path).map_err(|source| {
            ArchcastError::ReadInput {
                path: direct_path.to_path_buf(),
                source,
            }
        })?;
        let val: serde_json::Value = if direct_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        {
            serde_json::from_str(&raw).map_err(|source| ArchcastError::JsonParse {
                path: direct_path.to_path_buf(),
                source,
            })?
        } else {
            serde_yaml_ng::from_str(&raw).map_err(|source| ArchcastError::YamlParse {
                path: direct_path.to_path_buf(),
                source,
            })?
        };
        return merge_theme_value(val, themes_dir, depth);
    }

    // Check external registry directory first (allows external overrides of built-in IDs).
    let dir = resolve_themes_dir(themes_dir);
    for ext in ["yaml", "yml", "json"] {
        let candidate = dir.join(format!("{theme_ref}.{ext}"));
        if candidate.is_file() {
            let raw =
                std::fs::read_to_string(&candidate).map_err(|source| ArchcastError::ReadInput {
                    path: candidate.clone(),
                    source,
                })?;
            let val: serde_json::Value = if ext == "json" {
                serde_json::from_str(&raw).map_err(|source| ArchcastError::JsonParse {
                    path: candidate.clone(),
                    source,
                })?
            } else {
                serde_yaml_ng::from_str(&raw).map_err(|source| ArchcastError::YamlParse {
                    path: candidate.clone(),
                    source,
                })?
            };
            return merge_theme_value(val, themes_dir, depth);
        }
    }

    if let Some(built) = builtin_theme(theme_ref) {
        return Ok(built);
    }

    Err(ArchcastError::Usage(format!(
        "unknown theme '{theme_ref}'; run `archcast theme list` to see built-in and external themes (dir: {})",
        dir.display()
    )))
}

/// List all built-in and external themes in deterministic ID order.
pub fn list_themes(themes_dir: Option<&Path>) -> Result<Vec<ThemeSummary>> {
    let mut map: BTreeMap<String, ThemeSummary> = BTreeMap::new();
    for id in BUILTIN_THEME_IDS {
        if let Some(t) = builtin_theme(id) {
            map.insert(
                t.id.clone(),
                ThemeSummary {
                    id: t.id,
                    display_name: t.display_name,
                    color_scheme: t.color_scheme,
                    source: "builtin".to_string(),
                    overrides_builtin: false,
                    path: None,
                },
            );
        }
    }

    let dir = resolve_themes_dir(themes_dir);
    if dir.is_dir() {
        let entries = std::fs::read_dir(&dir).map_err(|source| ArchcastError::ReadInput {
            path: dir.clone(),
            source,
        })?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let is_cfg = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "yaml" | "yml" | "json"));
            if !is_cfg {
                continue;
            }
            if let Ok(t) = load_theme_file(&path, themes_dir) {
                let overrides = BUILTIN_THEME_IDS.contains(&t.id.as_str());
                map.insert(
                    t.id.clone(),
                    ThemeSummary {
                        id: t.id,
                        display_name: t.display_name,
                        color_scheme: t.color_scheme,
                        source: "external".to_string(),
                        overrides_builtin: overrides,
                        path: Some(path),
                    },
                );
            }
        }
    }

    Ok(map.into_values().collect())
}

/// Save a validated [`ThemeConfig`] to `<themes_dir>/<id>.yaml`.
pub fn save_external_theme(
    cfg: &ThemeConfig,
    themes_dir: Option<&Path>,
    allow_overwrite: bool,
) -> Result<PathBuf> {
    cfg.validate()?;
    let dir = resolve_themes_dir(themes_dir);
    let dest = dir.join(format!("{}.yaml", cfg.id));
    if dest.exists() && !allow_overwrite {
        return Err(ArchcastError::Usage(format!(
            "theme '{}' already exists at {} (use `theme update` or `--force`)",
            cfg.id,
            dest.display()
        )));
    }
    let yaml = theme_to_yaml(cfg)?;
    write_atomic(&dest, yaml.as_bytes())?;
    Ok(dest)
}

/// Delete an external theme file from `<themes_dir>/<id>.{yaml,yml,json}`.
pub fn delete_external_theme(id: &str, themes_dir: Option<&Path>) -> Result<(PathBuf, bool)> {
    validate_theme_id(id)?;
    let dir = resolve_themes_dir(themes_dir);
    for ext in ["yaml", "yml", "json"] {
        let candidate = dir.join(format!("{id}.{ext}"));
        if candidate.is_file() {
            std::fs::remove_file(&candidate).map_err(|source| ArchcastError::WriteOutput {
                path: candidate.clone(),
                source,
            })?;
            let restored_builtin = BUILTIN_THEME_IDS.contains(&id);
            return Ok((candidate, restored_builtin));
        }
    }
    if BUILTIN_THEME_IDS.contains(&id) {
        return Err(ArchcastError::Usage(format!(
            "theme '{id}' is a bundled built-in theme and cannot be deleted (no external override exists in {})",
            dir.display()
        )));
    }
    Err(ArchcastError::Usage(format!(
        "external theme '{id}' not found in {}",
        dir.display()
    )))
}

pub fn theme_to_yaml(cfg: &ThemeConfig) -> Result<String> {
    Ok(serde_yaml_ng::to_string(cfg)?)
}

fn parse_hex_rgba(hex: &str) -> Option<[u8; 4]> {
    let s = hex.trim().strip_prefix('#')?;
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some([r, g, b, 255])
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        let a = u8::from_str_radix(&s[6..8], 16).ok()?;
        Some([r, g, b, a])
    } else {
        None
    }
}

fn parse_hex_rgb(hex: &str) -> Option<[f32; 3]> {
    let [r, g, b, _] = parse_hex_rgba(hex)?;
    Some([r as f32, g as f32, b as f32])
}

// ---------------------------------------------------------------------------
// 5 Bundled Default Theme Presets
// ---------------------------------------------------------------------------

fn s(v: &str) -> String {
    v.to_string()
}

pub fn starlink_light() -> ThemeConfig {
    ThemeConfig {
        schema_version: THEME_SCHEMA_VERSION,
        id: s("starlink-light"),
        display_name: s("Starlink Light · 清爽评审白"),
        extends: None,
        color_scheme: s("light"),
        is_dark: false,
        surface: SurfaceTokens {
            page_bg: s("#eef2f7"),
            card_bg: s("#ffffff"),
            card_border: s("#dbe3ee"),
            card_shadow: s("0 1px 3px rgba(15,23,42,.05)"),
            svg_bg: s("#ffffff"),
            node_bg: s("#fdfefe"),
            node_border: s("#d7dee8"),
            subcard_bg: s("#ffffff"),
            subcard_border: s("#e2e8f0"),
            callout_bg: s("#fbfcfe"),
            callout_border: s("#d7dee8"),
            code_bg: s("#f1f5f9"),
            codebox_bg: s("#f8fafc"),
            codebox_border: s("#e2e8f0"),
        },
        ink: InkTokens {
            page_fg: s("#0f172a"),
            lead_fg: s("#475569"),
            note_fg: s("#64748b"),
            code_fg: s("#0f172a"),
            svg_title: s("#0f172a"),
            svg_subtitle: s("#6b7280"),
            svg_meta: s("#9aa4b2"),
            col_header: s("#94a3b8"),
            node_title: s("#1f2937"),
            node_text: s("#4b5563"),
            node_mono: s("#334155"),
            node_footer: s("#8b95a5"),
            subcard_title: s("#1f2937"),
        },
        chrome: ChromeTokens {
            toc_link: s("#0f8a5f"),
            h2_accent: s("#0f8a5f"),
            btn_bg: s("#f8fafc"),
            btn_fg: s("#334155"),
            btn_border: s("#cbd5e1"),
            btn_hover_bg: s("#eef2f7"),
            pill_bg: s("#0f8a5f1a"),
            pill_fg: s("#0f8a5f"),
            edge_spine: s("#cbd5e1"),
            edge_stroke: s("#94a3b8"),
            edge_label_bg: s("#ffffff"),
            edge_label_fg: s("#94a3b8"),
            badge_opacity: s("0.12"),
            ver_bg_alpha: s("1a"),
        },
        table: TableTokens {
            head_bg: s("#f4f6fa"),
            head_fg: s("#475569"),
            border: s("#e2e8f0"),
            row_border: s("#eef2f7"),
            hover_bg: s("#fafcff"),
            grp_bg: s("#f4f6fa"),
            grp_fg: s("#334155"),
            cnt_fg: s("#94a3b8"),
            small_fg: s("#64748b"),
            down_fg: s("#0f8a5f"),
        },
        warning: WarningTokens {
            hint_bg: s("#fff8ec"),
            hint_fg: s("#7c5a12"),
            hint_accent: s("#b45309"),
            banner_bg: s("#fff8ec"),
            banner_border: s("#f0d9b5"),
            banner_accent: s("#b45309"),
            banner_title: s("#b45309"),
            banner_body: s("#7a4a12"),
        },
        tones: ToneTokens {
            default: s("#334155"),
            muted: s("#8b95a5"),
            blue: s("#4b83d4"),
            orange: s("#e08b3c"),
            green: s("#3e9e6e"),
            emerald: s("#0f8a5f"),
            amber: s("#b45309"),
            purple: s("#7b61c9"),
            gold: s("#c9a227"),
            red: s("#d9534f"),
        },
        motion: MotionTokens {
            pulse_hex: [s("#0f8a5f"), s("#4b83d4"), s("#e08b3c"), s("#7b61c9")],
            beacon_red: s("#d9534f"),
            beacon_green: s("#0f8a5f"),
        },
    }
}

pub fn obsidian_neon() -> ThemeConfig {
    ThemeConfig {
        schema_version: THEME_SCHEMA_VERSION,
        id: s("obsidian-neon"),
        display_name: s("Obsidian Neon · 暗夜大屏科技风"),
        extends: None,
        color_scheme: s("dark"),
        is_dark: true,
        surface: SurfaceTokens {
            page_bg: s("#0b0f17"),
            card_bg: s("#111827"),
            card_border: s("#243247"),
            card_shadow: s("0 4px 20px rgba(0,0,0,.35)"),
            svg_bg: s("#0f1623"),
            node_bg: s("#151e2e"),
            node_border: s("#26354d"),
            subcard_bg: s("#1b263b"),
            subcard_border: s("#2d3f5c"),
            callout_bg: s("#141d2c"),
            callout_border: s("#26354d"),
            code_bg: s("#1e293b"),
            codebox_bg: s("#0d1320"),
            codebox_border: s("#233148"),
        },
        ink: InkTokens {
            page_fg: s("#f1f5f9"),
            lead_fg: s("#94a3b8"),
            note_fg: s("#94a3b8"),
            code_fg: s("#e2e8f0"),
            svg_title: s("#f8fafc"),
            svg_subtitle: s("#94a3b8"),
            svg_meta: s("#64748b"),
            col_header: s("#7c93b2"),
            node_title: s("#f1f5f9"),
            node_text: s("#cbd5e1"),
            node_mono: s("#93c5fd"),
            node_footer: s("#7c93b2"),
            subcard_title: s("#f1f5f9"),
        },
        chrome: ChromeTokens {
            toc_link: s("#34d399"),
            h2_accent: s("#34d399"),
            btn_bg: s("#1b263b"),
            btn_fg: s("#cbd5e1"),
            btn_border: s("#334155"),
            btn_hover_bg: s("#26354f"),
            pill_bg: s("#34d39924"),
            pill_fg: s("#34d399"),
            edge_spine: s("#334866"),
            edge_stroke: s("#526d91"),
            edge_label_bg: s("#0f1623"),
            edge_label_fg: s("#94a3b8"),
            badge_opacity: s("0.22"),
            ver_bg_alpha: s("28"),
        },
        table: TableTokens {
            head_bg: s("#162033"),
            head_fg: s("#cbd5e1"),
            border: s("#243247"),
            row_border: s("#1c273a"),
            hover_bg: s("#182338"),
            grp_bg: s("#162033"),
            grp_fg: s("#e2e8f0"),
            cnt_fg: s("#64748b"),
            small_fg: s("#94a3b8"),
            down_fg: s("#34d399"),
        },
        warning: WarningTokens {
            hint_bg: s("#2a1f0d"),
            hint_fg: s("#fcd34d"),
            hint_accent: s("#f59e0b"),
            banner_bg: s("#2a1f0d"),
            banner_border: s("#78350f"),
            banner_accent: s("#f59e0b"),
            banner_title: s("#fbbf24"),
            banner_body: s("#fde68a"),
        },
        tones: ToneTokens {
            default: s("#cbd5e1"),
            muted: s("#8a9bb2"),
            blue: s("#60a5fa"),
            orange: s("#fb923c"),
            green: s("#34d399"),
            emerald: s("#2dd4bf"),
            amber: s("#fbbf24"),
            purple: s("#a78bfa"),
            gold: s("#eab308"),
            red: s("#f87171"),
        },
        motion: MotionTokens {
            pulse_hex: [s("#34d399"), s("#60a5fa"), s("#fb923c"), s("#a78bfa")],
            beacon_red: s("#f87171"),
            beacon_green: s("#34d399"),
        },
    }
}

pub fn paper_editorial() -> ThemeConfig {
    ThemeConfig {
        schema_version: THEME_SCHEMA_VERSION,
        id: s("paper-editorial"),
        display_name: s("Paper Editorial · 暖纸杂志汇报风"),
        extends: None,
        color_scheme: s("light"),
        is_dark: false,
        surface: SurfaceTokens {
            page_bg: s("#f5f2eb"),
            card_bg: s("#fffdf9"),
            card_border: s("#e5dfd3"),
            card_shadow: s("0 1px 4px rgba(31,29,26,.05)"),
            svg_bg: s("#fffdf9"),
            node_bg: s("#fdfbf7"),
            node_border: s("#e2dbcd"),
            subcard_bg: s("#f8f4ec"),
            subcard_border: s("#e5dfd3"),
            callout_bg: s("#faf6ee"),
            callout_border: s("#e2dbcd"),
            code_bg: s("#f1ece2"),
            codebox_bg: s("#f3efe6"),
            codebox_border: s("#e2dbcd"),
        },
        ink: InkTokens {
            page_fg: s("#1f1d1a"),
            lead_fg: s("#57534e"),
            note_fg: s("#78716c"),
            code_fg: s("#292524"),
            svg_title: s("#1f1d1a"),
            svg_subtitle: s("#57534e"),
            svg_meta: s("#8c857b"),
            col_header: s("#8c857b"),
            node_title: s("#1f1d1a"),
            node_text: s("#44403c"),
            node_mono: s("#3f3a34"),
            node_footer: s("#8c857b"),
            subcard_title: s("#1f1d1a"),
        },
        chrome: ChromeTokens {
            toc_link: s("#c25e3a"),
            h2_accent: s("#c25e3a"),
            btn_bg: s("#f8f4ec"),
            btn_fg: s("#44403c"),
            btn_border: s("#d6cfc2"),
            btn_hover_bg: s("#efe9dd"),
            pill_bg: s("#c25e3a1a"),
            pill_fg: s("#c25e3a"),
            edge_spine: s("#d1c9bc"),
            edge_stroke: s("#a89f91"),
            edge_label_bg: s("#fffdf9"),
            edge_label_fg: s("#8c857b"),
            badge_opacity: s("0.14"),
            ver_bg_alpha: s("1c"),
        },
        table: TableTokens {
            head_bg: s("#f5f0e6"),
            head_fg: s("#57534e"),
            border: s("#e5dfd3"),
            row_border: s("#f0ebe0"),
            hover_bg: s("#faf7f0"),
            grp_bg: s("#f5f0e6"),
            grp_fg: s("#292524"),
            cnt_fg: s("#a8a29e"),
            small_fg: s("#78716c"),
            down_fg: s("#2e7d5b"),
        },
        warning: WarningTokens {
            hint_bg: s("#fdf3e3"),
            hint_fg: s("#7a4810"),
            hint_accent: s("#b86b1b"),
            banner_bg: s("#fdf3e3"),
            banner_border: s("#e8c999"),
            banner_accent: s("#b86b1b"),
            banner_title: s("#a35b10"),
            banner_body: s("#7a4810"),
        },
        tones: ToneTokens {
            default: s("#3f3a34"),
            muted: s("#8c857b"),
            blue: s("#3b6fa0"),
            orange: s("#c25e3a"),
            green: s("#2e7d5b"),
            emerald: s("#257051"),
            amber: s("#b86b1b"),
            purple: s("#7c5fa3"),
            gold: s("#a6822a"),
            red: s("#c8433e"),
        },
        motion: MotionTokens {
            pulse_hex: [s("#c25e3a"), s("#2e7d5b"), s("#3b6fa0"), s("#7c5fa3")],
            beacon_red: s("#c8433e"),
            beacon_green: s("#2e7d5b"),
        },
    }
}

pub fn blueprint_ink() -> ThemeConfig {
    ThemeConfig {
        schema_version: THEME_SCHEMA_VERSION,
        id: s("blueprint-ink"),
        display_name: s("Blueprint Ink · 工程蓝图图纸风"),
        extends: None,
        color_scheme: s("light"),
        is_dark: false,
        surface: SurfaceTokens {
            page_bg: s("#ebf0f7"),
            card_bg: s("#f8fafd"),
            card_border: s("#c7d7ea"),
            card_shadow: s("0 1px 3px rgba(15,41,66,.06)"),
            svg_bg: s("#f8fafd"),
            node_bg: s("#ffffff"),
            node_border: s("#c7d7ea"),
            subcard_bg: s("#f3f7fc"),
            subcard_border: s("#cfe0f2"),
            callout_bg: s("#ffffff"),
            callout_border: s("#c7d7ea"),
            code_bg: s("#e6eef8"),
            codebox_bg: s("#eef4fb"),
            codebox_border: s("#cfe0f2"),
        },
        ink: InkTokens {
            page_fg: s("#0f2942"),
            lead_fg: s("#33506e"),
            note_fg: s("#4e6a87"),
            code_fg: s("#0f2942"),
            svg_title: s("#0f2942"),
            svg_subtitle: s("#33506e"),
            svg_meta: s("#6b84a0"),
            col_header: s("#5b7fa6"),
            node_title: s("#0f2942"),
            node_text: s("#33506e"),
            node_mono: s("#1e3a5f"),
            node_footer: s("#6b84a0"),
            subcard_title: s("#0f2942"),
        },
        chrome: ChromeTokens {
            toc_link: s("#1d4ed8"),
            h2_accent: s("#1d4ed8"),
            btn_bg: s("#eef4fb"),
            btn_fg: s("#1e3a5f"),
            btn_border: s("#b8cce3"),
            btn_hover_bg: s("#e1ecf8"),
            pill_bg: s("#1d4ed81a"),
            pill_fg: s("#1d4ed8"),
            edge_spine: s("#9bb7d6"),
            edge_stroke: s("#5b7fa6"),
            edge_label_bg: s("#f8fafd"),
            edge_label_fg: s("#4e6a87"),
            badge_opacity: s("0.13"),
            ver_bg_alpha: s("1a"),
        },
        table: TableTokens {
            head_bg: s("#edf3fa"),
            head_fg: s("#1e3a5f"),
            border: s("#c7d7ea"),
            row_border: s("#e3ecf6"),
            hover_bg: s("#f2f7fd"),
            grp_bg: s("#e9f1fa"),
            grp_fg: s("#0f2942"),
            cnt_fg: s("#6b84a0"),
            small_fg: s("#4e6a87"),
            down_fg: s("#0d9488"),
        },
        warning: WarningTokens {
            hint_bg: s("#fef7e6"),
            hint_fg: s("#7c4a03"),
            hint_accent: s("#b45309"),
            banner_bg: s("#fef7e6"),
            banner_border: s("#e6c27a"),
            banner_accent: s("#b45309"),
            banner_title: s("#9a4a04"),
            banner_body: s("#7c4a03"),
        },
        tones: ToneTokens {
            default: s("#1e3a5f"),
            muted: s("#6b84a0"),
            blue: s("#1d4ed8"),
            orange: s("#d97706"),
            green: s("#0f766e"),
            emerald: s("#0d9488"),
            amber: s("#b45309"),
            purple: s("#6366f1"),
            gold: s("#ca8a04"),
            red: s("#dc2626"),
        },
        motion: MotionTokens {
            pulse_hex: [s("#1d4ed8"), s("#0d9488"), s("#6366f1"), s("#d97706")],
            beacon_red: s("#dc2626"),
            beacon_green: s("#0d9488"),
        },
    }
}

pub fn catppuccin_mocha() -> ThemeConfig {
    ThemeConfig {
        schema_version: THEME_SCHEMA_VERSION,
        id: s("catppuccin-mocha"),
        display_name: s("Catppuccin Mocha · 护眼粉彩暗色"),
        extends: None,
        color_scheme: s("dark"),
        is_dark: true,
        surface: SurfaceTokens {
            page_bg: s("#11111b"),
            card_bg: s("#1e1e2e"),
            card_border: s("#313244"),
            card_shadow: s("0 4px 18px rgba(0,0,0,.32)"),
            svg_bg: s("#1e1e2e"),
            node_bg: s("#24273a"),
            node_border: s("#45475a"),
            subcard_bg: s("#313244"),
            subcard_border: s("#45475a"),
            callout_bg: s("#24273a"),
            callout_border: s("#45475a"),
            code_bg: s("#313244"),
            codebox_bg: s("#181825"),
            codebox_border: s("#313244"),
        },
        ink: InkTokens {
            page_fg: s("#cdd6f4"),
            lead_fg: s("#a6adc8"),
            note_fg: s("#a6adc8"),
            code_fg: s("#cdd6f4"),
            svg_title: s("#cdd6f4"),
            svg_subtitle: s("#bac2de"),
            svg_meta: s("#9399b2"),
            col_header: s("#9399b2"),
            node_title: s("#cdd6f4"),
            node_text: s("#bac2de"),
            node_mono: s("#89dceb"),
            node_footer: s("#9399b2"),
            subcard_title: s("#cdd6f4"),
        },
        chrome: ChromeTokens {
            toc_link: s("#a6e3a1"),
            h2_accent: s("#a6e3a1"),
            btn_bg: s("#313244"),
            btn_fg: s("#cdd6f4"),
            btn_border: s("#45475a"),
            btn_hover_bg: s("#45475a"),
            pill_bg: s("#a6e3a124"),
            pill_fg: s("#a6e3a1"),
            edge_spine: s("#585b70"),
            edge_stroke: s("#7f849c"),
            edge_label_bg: s("#1e1e2e"),
            edge_label_fg: s("#a6adc8"),
            badge_opacity: s("0.20"),
            ver_bg_alpha: s("28"),
        },
        table: TableTokens {
            head_bg: s("#181825"),
            head_fg: s("#bac2de"),
            border: s("#313244"),
            row_border: s("#262637"),
            hover_bg: s("#252638"),
            grp_bg: s("#181825"),
            grp_fg: s("#cdd6f4"),
            cnt_fg: s("#7f849c"),
            small_fg: s("#a6adc8"),
            down_fg: s("#a6e3a1"),
        },
        warning: WarningTokens {
            hint_bg: s("#2e261b"),
            hint_fg: s("#f9e2af"),
            hint_accent: s("#fab387"),
            banner_bg: s("#2e261b"),
            banner_border: s("#6e5636"),
            banner_accent: s("#fab387"),
            banner_title: s("#f9e2af"),
            banner_body: s("#f5e0dc"),
        },
        tones: ToneTokens {
            default: s("#bac2de"),
            muted: s("#9399b2"),
            blue: s("#89b4fa"),
            orange: s("#fab387"),
            green: s("#a6e3a1"),
            emerald: s("#94e2d5"),
            amber: s("#f9e2af"),
            purple: s("#cba6f7"),
            gold: s("#eed49f"),
            red: s("#f38ba8"),
        },
        motion: MotionTokens {
            pulse_hex: [s("#a6e3a1"), s("#89b4fa"), s("#fab387"), s("#cba6f7")],
            beacon_red: s("#f38ba8"),
            beacon_green: s("#a6e3a1"),
        },
    }
}
