use crate::error::{AppError, Result};
#[allow(unused_imports)]
use crate::output;
use crate::pack::{self, PackOptions};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
// ─── Background pack result ───

enum BackgroundPackResult {
    Success {
        message: String,
        atlas_image: image::RgbaImage,
        sprites: Vec<SpriteInfo>,
        atlas_w: u32,
        atlas_h: u32,
    },
    Error(String),
}

// ─── Project file (.tpproj) ───

// PartialEq is required by the undo/redo `History` snapshot diffing — we
// compare last-recorded vs current project every frame. Project + sub-types
// are tiny, so the comparison is cheap.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Project {
    /// Project format version
    pub version: u32,
    /// Atlas output name
    pub output_name: String,
    /// Output directory (empty = same as project file dir)
    pub output_dir: String,
    /// All sprite file paths (absolute)
    pub sprites: Vec<String>,
    /// Packing settings
    pub settings: ProjectSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProjectSettings {
    pub max_size: usize,
    pub spacing: u32,
    pub padding: u32,
    pub extrude: u32,
    pub trim: bool,
    pub trim_threshold: u8,
    pub rotate: bool,
    pub pot: bool,
    pub polygon: bool,
    pub tolerance: f32,
    pub quantize: bool,
    pub quantize_quality: u8,
    pub format_idx: usize,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: 1,
            output_name: "atlas".to_string(),
            output_dir: String::new(),
            sprites: Vec::new(),
            settings: ProjectSettings::default(),
        }
    }
}

impl Default for ProjectSettings {
    fn default() -> Self {
        Self {
            max_size: 4096,
            spacing: 0,
            padding: 0,
            extrude: 0,
            trim: true,
            trim_threshold: 0,
            rotate: true,
            pot: true,
            polygon: false,
            tolerance: 2.0,
            quantize: false,
            quantize_quality: 85,
            format_idx: 0,
        }
    }
}

impl Project {
    fn save(&self, path: &Path) -> std::result::Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, json).map_err(|e| e.to_string())
    }

    fn load(path: &Path) -> std::result::Result<Self, String> {
        let content = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }
}

// ─── Sprite info parsed from output ───

struct SpriteInfo {
    name: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    rotated: bool,
    source_w: f32,
    source_h: f32,
    /// Polygon mesh — atlas-space vertex positions (verticesUV from json output).
    /// `None` when the atlas was not packed in polygon mode.
    mesh_vertices: Option<Vec<[f32; 2]>>,
    /// Triangle indices into `mesh_vertices`.
    mesh_triangles: Option<Vec<[usize; 3]>>,
}

// ─── Alert / Toast system ───

#[derive(Clone)]
struct Toast {
    message: String,
    kind: ToastKind,
    created: std::time::Instant,
}

#[derive(Clone, Copy, PartialEq)]
#[allow(dead_code)]
enum ToastKind {
    Info,
    Success,
    Error,
}

impl Toast {
    fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self {
            message: message.into(),
            kind,
            created: std::time::Instant::now(),
        }
    }

    fn color(&self) -> egui::Color32 {
        match self.kind {
            ToastKind::Info => egui::Color32::from_rgb(60, 130, 220),
            ToastKind::Success => egui::Color32::from_rgb(50, 180, 80),
            ToastKind::Error => egui::Color32::from_rgb(220, 60, 60),
        }
    }

    fn is_expired(&self) -> bool {
        self.created.elapsed().as_secs_f32() > 5.0
    }
}

// ─── Dialog types ───

enum Dialog {
    About,
    License,
    ExportAs,
    UnsavedChanges(Box<MenuAction>),
}

// ─── App modes ───

enum AppMode {
    Welcome,
    Packer(PackerState),
    Viewer(ViewerState),
}

struct PackerState {
    project: Project,
    project_path: Option<PathBuf>,
    dirty: bool,
    thumbnails: HashMap<String, (egui::TextureHandle, u32, u32)>,
    search_text: String,
    status: PackStatus,
    preview: Option<InlinePreview>,
    split_dir: SplitDir,
    auto_pack: bool,
    /// Receiver for background pack results
    pack_rx: Option<std::sync::mpsc::Receiver<BackgroundPackResult>>,
    /// Auto-pack queued by an event (drop/add/delete/settings change). Honored
    /// only when `auto_pack` is on; otherwise just sits dirty until the user
    /// clicks Pack!.
    needs_auto_pack: bool,
    /// User clicked Pack! — bypass the `auto_pack` gate. Cleared after the
    /// pack thread is launched. Separate from `needs_auto_pack` so a user
    /// who turned auto-pack OFF can still manually trigger packs.
    manual_pack: bool,
    /// Undo / redo history. Snapshots the project on every frame where the
    /// state has actually changed since the last snapshot — driven by
    /// `History::maybe_record` from the update loop, NOT by per-action
    /// callsites (which would be easy to miss).
    history: History,
}

/// Linear undo / redo history. `states` always contains at least one entry
/// (the initial project); `cursor` points at the "present". Undo decrements
/// the cursor and reads back; redo increments it.
struct History {
    states: Vec<Project>,
    cursor: usize,
    /// Mirrors the present so we can detect changes without running PartialEq
    /// on every field every frame; we only re-clone when the user actually
    /// edits something.
    last_recorded: Project,
}

/// Cap to keep memory bounded — a Project is small, but in a marathon
/// session of drag-drops the snapshot list could otherwise grow unbounded.
const HISTORY_LIMIT: usize = 50;

impl History {
    fn new(initial: Project) -> Self {
        Self {
            last_recorded: initial.clone(),
            states: vec![initial],
            cursor: 0,
        }
    }

    /// Called once per frame from `ui_packer`. If the project diverged from
    /// the last snapshot, push a new entry, drop any redo states beyond the
    /// cursor, and bump the cursor.
    fn maybe_record(&mut self, current: &Project) {
        if *current == self.last_recorded {
            return;
        }
        // Branching off — anything ahead of the cursor is now invalid.
        self.states.truncate(self.cursor + 1);
        self.states.push(current.clone());
        self.cursor = self.states.len() - 1;
        self.last_recorded = current.clone();
        // Bound history depth from the BACK so the most recent edits survive.
        while self.states.len() > HISTORY_LIMIT {
            self.states.remove(0);
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    /// Step back one snapshot. Returns the project to restore, or `None` when
    /// already at the oldest entry (Cmd+Z is a no-op then).
    fn undo(&mut self) -> Option<Project> {
        if self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.last_recorded = self.states[self.cursor].clone();
        Some(self.states[self.cursor].clone())
    }

    fn redo(&mut self) -> Option<Project> {
        if self.cursor + 1 >= self.states.len() {
            return None;
        }
        self.cursor += 1;
        self.last_recorded = self.states[self.cursor].clone();
        Some(self.states[self.cursor].clone())
    }

    fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    fn can_redo(&self) -> bool {
        self.cursor + 1 < self.states.len()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SplitDir {
    Horizontal, // left sprites | right preview
    Vertical,   // top sprites | bottom preview
}

struct InlinePreview {
    texture: Option<egui::TextureHandle>,
    image: image::RgbaImage,
    sprites: Vec<SpriteInfo>,
    atlas_w: u32,
    atlas_h: u32,
    zoom: f32,
    pan_offset: egui::Vec2,
    hovered: Option<usize>,
    show_grid: bool,
    show_names: bool,
    /// Polygon-mesh overlay toggle (only meaningful when atlas was packed with --polygon).
    show_mesh: bool,
    /// First frame: auto-fit zoom
    needs_fit: bool,
}

enum PackStatus {
    Idle,
    Success(String),
    Error(String),
}

struct ViewerState {
    atlas_texture: Option<egui::TextureHandle>,
    atlas_image: image::RgbaImage,
    sprites: Vec<SpriteInfo>,
    animations: HashMap<String, Vec<String>>,
    atlas_name: String,
    atlas_w: u32,
    atlas_h: u32,
    zoom: f32,
    pan_offset: egui::Vec2,
    hovered_sprite: Option<usize>,
    selected_sprite: Option<usize>,
    show_grid: bool,
    show_names: bool,
    show_mesh: bool,
    search_text: String,
    source_path: String,
}

// ─── Main App ───

struct MJAtlasApp {
    mode: AppMode,
    toasts: Vec<Toast>,
    open_dialog: Option<Dialog>,
    recent_files: Vec<PathBuf>,
    dark_mode: bool,
    fonts_loaded: bool,
    /// Per-user config (hfrog mirror settings, etc). Loaded once on startup;
    /// the settings panel writes back through `crate::config::Config::save`
    /// when the user clicks "Save settings".
    config: crate::config::Config,
    /// Buffer values shown in the hfrog panel before the user hits "Save".
    /// Kept separate from `config.hfrog` so canceling/discarding edits is
    /// trivial — just re-read from `config`.
    config_dirty: bool,
    /// hfrog connection state — `Probing` at startup, then resolves to
    /// Online/Offline within the probe timeout (1.5 s). Drives the menubar
    /// badge and the cloud-side project list.
    connection: crate::connection::ConnectionState,
    /// Channel to receive the result of an in-flight probe. Polled each
    /// frame; cleared once the result arrives.
    probe_rx: Option<std::sync::mpsc::Receiver<crate::connection::ProbeResult>>,
    /// Cached cloud project list (refreshed when entering Welcome screen
    /// while online, or after a manual Refresh click). Empty when offline
    /// or when the read failed.
    cloud_projects: Vec<crate::hfrog::CloudProject>,
    /// In-flight cloud-list refresh — same pattern as `probe_rx`.
    cloud_list_rx:
        Option<std::sync::mpsc::Receiver<std::result::Result<Vec<crate::hfrog::CloudProject>, String>>>,
}

const FORMAT_NAMES: &[(&str, &str)] = &[
    ("json", "TexturePacker JSON Hash"),
    ("json-array", "TexturePacker JSON Array"),
    ("godot-tpsheet", "Godot .tpsheet"),
    ("godot-tres", "Godot native .tres"),
];

const VERSION: &str = env!("CARGO_PKG_VERSION");

impl Default for MJAtlasApp {
    fn default() -> Self {
        // Best-effort config load — first run / parse error both fall back
        // to defaults rather than crashing the GUI.
        let config = crate::config::Config::load().unwrap_or_else(|e| {
            log::warn!("config: load failed, using defaults: {}", e);
            crate::config::Config::default()
        });
        // Kick off a hfrog reachability probe immediately. The receiver is
        // polled each update() frame and resolves within 1.5 s; meanwhile
        // the UI shows a spinner and the user can still work locally —
        // nothing in the boot path blocks on the result.
        let probe_rx = Some(crate::connection::spawn_probe(&config.hfrog));
        let mut connection = crate::connection::ConnectionState::default();
        connection.probed_endpoint = config.hfrog.endpoint.clone();
        // Land on Welcome screen so the cloud / local project list is the
        // first thing the user sees once the probe resolves.
        Self {
            mode: AppMode::Welcome,
            toasts: Vec::new(),
            open_dialog: None,
            recent_files: Vec::new(),
            dark_mode: false,
            fonts_loaded: false,
            config,
            config_dirty: false,
            connection,
            probe_rx,
            cloud_projects: Vec::new(),
            cloud_list_rx: None,
        }
    }
}

impl PackerState {
    fn new_empty() -> Self {
        let project = Project::default();
        Self {
            history: History::new(project.clone()),
            project,
            project_path: None,
            dirty: false,
            thumbnails: HashMap::new(),
            search_text: String::new(),
            status: PackStatus::Idle,
            preview: None,
            split_dir: SplitDir::Horizontal,
            auto_pack: true,
            pack_rx: None,
            needs_auto_pack: false,
            manual_pack: false,
        }
    }

    fn from_project(project: Project, path: PathBuf) -> Self {
        let needs_pack = !project.sprites.is_empty();
        Self {
            history: History::new(project.clone()),
            project,
            project_path: Some(path),
            dirty: false,
            thumbnails: HashMap::new(),
            search_text: String::new(),
            status: PackStatus::Idle,
            preview: None,
            split_dir: SplitDir::Horizontal,
            auto_pack: true,
            pack_rx: None,
            needs_auto_pack: needs_pack,
            manual_pack: false,
        }
    }

    #[allow(dead_code)]
    fn is_packing(&self) -> bool {
        self.pack_rx.is_some()
    }

    fn title(&self) -> String {
        let name = self.project_path.as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Untitled".to_string());
        if self.dirty {
            format!("{}*", name)
        } else {
            name
        }
    }
}

/// Load Inter + JetBrains Mono fonts into egui, plus the first available
/// system CJK font as a fallback for both families.
///
/// Why scan system paths instead of embedding a CJK font:
///   - A full Noto Sans CJK adds ~10 MB to the binary (currently ~1.5 MB).
///     Even subsets are 2-5 MB. The only reliable answer is "use whatever
///     the OS already has".
///   - macOS / Windows / mainstream Linux desktops all ship a CJK font at
///     a known path. We just read the first one that exists.
///   - Zero new dependencies (no font-kit / fontdb / sysfonts crate).
///   - When NOTHING is found (rare — headless CI containers), the fallback
///     is the existing tofu rendering plus a clear warning in the runlog.
///
/// The CJK font is appended LAST in each family's fallback list so Latin
/// glyphs always go through Inter / JetBrains Mono first; egui's text
/// shaper falls through to the next font only for missing glyphs.
fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Inter Regular — UI proportional font
    fonts.font_data.insert(
        "inter".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))
            .into(),
    );
    // Inter Bold
    fonts.font_data.insert(
        "inter_bold".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Inter-Bold.ttf"))
            .into(),
    );
    // JetBrains Mono — monospace
    fonts.font_data.insert(
        "jetbrains".to_owned(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"))
            .into(),
    );

    // Primary fonts: Inter for proportional, JetBrains Mono for monospace.
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains".to_owned());

    // CJK fallback: scan OS-installed fonts and use the first available.
    // Append to BOTH families so monospace text (file paths, sprite names)
    // also renders Chinese / Japanese / Korean correctly when those chars
    // appear inline.
    if let Some((handle, label)) = load_system_cjk_font() {
        fonts.font_data.insert("cjk".to_owned(), handle.into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("cjk".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
        log::info!("loaded CJK fallback font: {}", label);
    } else {
        log::warn!(
            "no system CJK font found at known paths; Chinese / Japanese / \
             Korean text will render as tofu (□). Install Noto Sans CJK or a \
             distro-equivalent to fix."
        );
    }

    ctx.set_fonts(fonts);
}

/// Try to read a system CJK font from a list of well-known platform paths.
/// Returns the font bytes wrapped in `egui::FontData` plus a human label
/// for the runlog, or `None` if no candidate file exists.
fn load_system_cjk_font() -> Option<(egui::FontData, String)> {
    // Order matters: prefer Sans over Serif, prefer Light/Regular weights
    // (Light renders well at small UI sizes), prefer broader-coverage
    // collections (CJK over SC-only). `.ttc` files are TrueType collections;
    // egui FontData's default index 0 picks the first font in the file —
    // that's "PingFang SC Regular" on macOS, "Microsoft YaHei UI Regular" on
    // Windows, "Noto Sans CJK SC Regular" on Linux. All acceptable for UI.
    let candidates: &[(&str, &str)] = &[
        // macOS — PingFang lived at /System/Library/Fonts/PingFang.ttc on
        // older releases but moved to Supplemental/ on macOS 14+, while older
        // installs may not have it at all. STHeiti is shipped on every
        // version since 10.6 and renders cleanly as a Sans fallback.
        ("/System/Library/Fonts/Supplemental/PingFang.ttc", "PingFang (macOS Supplemental)"),
        ("/System/Library/Fonts/PingFang.ttc", "PingFang (macOS)"),
        ("/System/Library/Fonts/STHeiti Light.ttc", "STHeiti Light (macOS)"),
        ("/System/Library/Fonts/STHeiti Medium.ttc", "STHeiti Medium (macOS)"),
        ("/System/Library/Fonts/Hiragino Sans GB.ttc", "Hiragino Sans GB (macOS)"),
        ("/System/Library/Fonts/Supplemental/Songti.ttc", "Songti (macOS Supplemental)"),
        // Windows 10 / 11
        ("C:/Windows/Fonts/msyh.ttc", "Microsoft YaHei (Windows)"),
        ("C:/Windows/Fonts/msyh.ttf", "Microsoft YaHei (Windows)"),
        ("C:/Windows/Fonts/simhei.ttf", "SimHei (Windows)"),
        ("C:/Windows/Fonts/simsun.ttc", "SimSun (Windows)"),
        ("C:/Windows/Fonts/Deng.ttf", "DengXian (Windows)"),
        // Linux — ordered by typical "universal" availability
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "Noto Sans CJK (Linux)"),
        ("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc", "Noto Sans CJK (Linux)"),
        ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", "Noto Sans CJK (Linux)"),
        ("/usr/share/fonts/wqy-microhei/wqy-microhei.ttc", "WenQuanYi Micro Hei (Linux)"),
        ("/usr/share/fonts/wqy-zenhei/wqy-zenhei.ttc", "WenQuanYi Zen Hei (Linux)"),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", "WenQuanYi Micro Hei (Linux)"),
        // ~/.fonts and ~/.local/share/fonts are user-local conventions on Linux;
        // skipping them here — if the user installed CJK only there, we'll log
        // the warning and they can copy or symlink to a system path.
    ];

    for (path, label) in candidates {
        if !std::path::Path::new(path).is_file() {
            continue;
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                return Some((egui::FontData::from_owned(bytes), label.to_string()));
            }
            Err(e) => {
                log::debug!("CJK candidate {} unreadable: {}", path, e);
                continue;
            }
        }
    }
    None
}

/// Apply a clean light theme inspired by Zed / One Light.
fn apply_light_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();

    // Background colors — warm white like Zed One Light
    let bg = egui::Color32::from_rgb(250, 250, 250);
    let panel_bg = egui::Color32::from_rgb(244, 244, 246);
    let widget_bg = egui::Color32::from_rgb(237, 238, 242);
    let accent = egui::Color32::from_rgb(56, 132, 244); // Blue accent
    let text = egui::Color32::from_rgb(36, 41, 51);
    let _text_dim = egui::Color32::from_rgb(120, 125, 136);
    let border = egui::Color32::from_rgb(218, 220, 224);

    visuals.panel_fill = panel_bg;
    visuals.window_fill = bg;
    visuals.extreme_bg_color = egui::Color32::WHITE;
    visuals.faint_bg_color = egui::Color32::from_rgb(245, 245, 247);

    // Widget rounding — slightly rounded, modern feel
    let rounding = egui::CornerRadius::same(6);

    visuals.widgets.noninteractive.bg_fill = panel_bg;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, border);
    visuals.widgets.noninteractive.corner_radius = rounding;

    visuals.widgets.inactive.bg_fill = widget_bg;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, border);
    visuals.widgets.inactive.corner_radius = rounding;

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(225, 228, 235);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent);
    visuals.widgets.hovered.corner_radius = rounding;

    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = rounding;

    visuals.widgets.open.bg_fill = egui::Color32::from_rgb(230, 232, 238);
    visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.open.corner_radius = rounding;

    visuals.selection.bg_fill = egui::Color32::from_rgb(200, 220, 252);
    visuals.selection.stroke = egui::Stroke::new(1.0, accent);

    visuals.window_shadow = egui::Shadow {
        offset: [0, 4].into(),
        blur: 12,
        spread: 0,
        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 25),
    };

    visuals.window_stroke = egui::Stroke::new(0.5, border);
    visuals.window_corner_radius = egui::CornerRadius::same(10);

    visuals.override_text_color = Some(text);

    ctx.set_visuals(visuals);

    // Slightly larger text for readability
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(18.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}

/// Apply a clean dark theme.
fn apply_dark_theme(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();

    let bg = egui::Color32::from_rgb(30, 32, 38);
    let panel_bg = egui::Color32::from_rgb(36, 39, 46);
    let widget_bg = egui::Color32::from_rgb(48, 52, 62);
    let accent = egui::Color32::from_rgb(86, 156, 255);
    let text = egui::Color32::from_rgb(220, 222, 228);
    let border = egui::Color32::from_rgb(58, 62, 72);

    let rounding = egui::CornerRadius::same(6);

    visuals.panel_fill = panel_bg;
    visuals.window_fill = bg;
    visuals.extreme_bg_color = egui::Color32::from_rgb(22, 24, 28);
    visuals.faint_bg_color = egui::Color32::from_rgb(40, 43, 50);

    visuals.widgets.noninteractive.bg_fill = panel_bg;
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, border);
    visuals.widgets.noninteractive.corner_radius = rounding;

    visuals.widgets.inactive.bg_fill = widget_bg;
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, border);
    visuals.widgets.inactive.corner_radius = rounding;

    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 65, 78);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent);
    visuals.widgets.hovered.corner_radius = rounding;

    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
    visuals.widgets.active.corner_radius = rounding;

    visuals.selection.bg_fill = egui::Color32::from_rgb(45, 65, 100);
    visuals.selection.stroke = egui::Stroke::new(1.0, accent);

    visuals.window_shadow = egui::Shadow {
        offset: [0, 6].into(),
        blur: 16,
        spread: 0,
        color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 60),
    };

    visuals.window_stroke = egui::Stroke::new(0.5, border);
    visuals.window_corner_radius = egui::CornerRadius::same(10);

    visuals.override_text_color = Some(text);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(18.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(11.0, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(13.0, egui::FontFamily::Monospace),
    );
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}

impl MJAtlasApp {
    fn with_viewer(viewer: ViewerState) -> Self {
        let mut app = Self::default();
        app.mode = AppMode::Viewer(viewer);
        app
    }

    fn toast(&mut self, msg: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast::new(msg, kind));
    }

    fn open_atlas_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Atlas files", &["json", "tpsheet"])
            .pick_file()
        {
            match load_atlas_for_viewer(&path) {
                Ok(viewer) => {
                    self.add_recent(&path);
                    self.mode = AppMode::Viewer(viewer);
                }
                Err(e) => {
                    self.toast(format!("Failed to open: {}", e), ToastKind::Error);
                }
            }
        }
    }

    fn add_recent(&mut self, path: &Path) {
        let path = path.to_path_buf();
        self.recent_files.retain(|p| p != &path);
        self.recent_files.insert(0, path);
        if self.recent_files.len() > 8 {
            self.recent_files.truncate(8);
        }
    }

    // ─── Global menu bar (shown on ALL screens) ───

    fn ui_menubar(&mut self, ctx: &egui::Context) {
        let mut action: Option<MenuAction> = None;
        let in_packer = matches!(self.mode, AppMode::Packer(_));
        let in_viewer = matches!(self.mode, AppMode::Viewer(_));

        // Helper that builds a menu Button labelled with a shortcut hint on
        // the right (egui's `Button::shortcut_text` renders the second arg
        // dimmed). Same convention macOS / Windows / Linux use natively.
        fn item(ui: &mut egui::Ui, label: &str, shortcut: &str) -> bool {
            ui.add(egui::Button::new(label).shortcut_text(shortcut))
                .clicked()
        }
        let cmd_label = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };

        egui::TopBottomPanel::top("main_menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if item(ui, "New Project", &format!("{}+N", cmd_label)) {
                        action = Some(MenuAction::NewProject);
                        ui.close_menu();
                    }
                    if item(ui, "Open Project...", &format!("{}+O", cmd_label)) {
                        action = Some(MenuAction::OpenProject);
                        ui.close_menu();
                    }

                    ui.separator();

                    ui.add_enabled_ui(in_packer, |ui| {
                        if item(ui, "Save Project", &format!("{}+S", cmd_label)) {
                            action = Some(MenuAction::SaveProject);
                            ui.close_menu();
                        }
                        if item(ui, "Save Project As...", &format!("{}+Shift+S", cmd_label))
                        {
                            action = Some(MenuAction::SaveProjectAs);
                            ui.close_menu();
                        }
                    });

                    ui.separator();

                    ui.add_enabled_ui(in_packer, |ui| {
                        if ui.button("Add Sprites...").clicked() {
                            action = Some(MenuAction::AddSprites);
                            ui.close_menu();
                        }
                    });

                    if ui.button("Open Atlas Preview...").clicked() {
                        action = Some(MenuAction::OpenAtlas);
                        ui.close_menu();
                    }

                    if !self.recent_files.is_empty() {
                        ui.menu_button("Open Recent", |ui| {
                            for path in self.recent_files.clone() {
                                let label = path
                                    .file_name()
                                    .map(|f| f.to_string_lossy().to_string())
                                    .unwrap_or_default();
                                if ui.button(&label).clicked() {
                                    action = Some(MenuAction::OpenRecent(path));
                                    ui.close_menu();
                                }
                            }
                        });
                    }

                    ui.separator();

                    ui.add_enabled_ui(in_viewer, |ui| {
                        if item(ui, "Export As...", &format!("{}+E", cmd_label)) {
                            action = Some(MenuAction::ExportAs);
                            ui.close_menu();
                        }
                    });

                    ui.separator();

                    ui.add_enabled_ui(in_packer || in_viewer, |ui| {
                        if item(ui, "Close (back to Welcome)", &format!("{}+W", cmd_label))
                        {
                            action = Some(MenuAction::GoHome);
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("Edit", |ui| {
                    let (can_undo, can_redo) = match &self.mode {
                        AppMode::Packer(s) => (s.history.can_undo(), s.history.can_redo()),
                        _ => (false, false),
                    };
                    ui.add_enabled_ui(can_undo, |ui| {
                        if item(ui, "Undo", &format!("{}+Z", cmd_label)) {
                            action = Some(MenuAction::Undo);
                            ui.close_menu();
                        }
                    });
                    ui.add_enabled_ui(can_redo, |ui| {
                        if item(ui, "Redo", &format!("{}+Shift+Z", cmd_label)) {
                            action = Some(MenuAction::Redo);
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("View", |ui| {
                    if ui.checkbox(&mut self.dark_mode, "Dark Mode").changed() {
                        ui.close_menu();
                    }
                });

                ui.add_enabled_ui(in_packer, |ui| {
                    if item(
                        ui,
                        "Pack",
                        &format!("{}+P", cmd_label),
                    ) {
                        action = Some(MenuAction::PackNow);
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About mj_atlas").clicked() {
                        action = Some(MenuAction::ShowAbout);
                        ui.close_menu();
                    }
                    if ui.button("License").clicked() {
                        action = Some(MenuAction::ShowLicense);
                        ui.close_menu();
                    }
                });

                // Right-aligned project info + connection mode badge.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status = match &self.mode {
                        AppMode::Welcome => "Home".to_string(),
                        AppMode::Packer(s) => format!("{} | {} sprites", s.title(), s.project.sprites.len()),
                        AppMode::Viewer(_) => "Preview".to_string(),
                    };
                    ui.label(
                        egui::RichText::new(format!("mj_atlas v{} | {}", VERSION, status))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                    ui.separator();

                    // Connection badge. Probing → spinner; online → green
                    // dot + endpoint short-form; offline → grey dot + retry
                    // button. Click on a non-probing badge re-runs the probe.
                    use crate::connection::ConnectionMode;
                    match self.connection.mode {
                        ConnectionMode::Probing => {
                            ui.spinner();
                            ui.label(
                                egui::RichText::new("Probing hfrog…")
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                        }
                        ConnectionMode::Online => {
                            // Show "● Online · <host>" with retry on click.
                            let host = host_of(&self.connection.probed_endpoint);
                            let resp = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!("● Online · {}", host))
                                        .small()
                                        .color(egui::Color32::from_rgb(60, 180, 90)),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if resp.on_hover_text("click to refresh").clicked() {
                                self.retry_probe();
                                self.refresh_cloud_projects();
                            }
                        }
                        ConnectionMode::Offline => {
                            let resp = ui.add(
                                egui::Label::new(
                                    egui::RichText::new("○ Offline")
                                        .small()
                                        .color(egui::Color32::from_rgb(180, 100, 60)),
                                )
                                .sense(egui::Sense::click()),
                            );
                            let tt = if self.connection.last_error.is_empty() {
                                "click to try cloud".to_string()
                            } else {
                                format!("{}\nclick to retry", self.connection.last_error)
                            };
                            if resp.on_hover_text(tt).clicked() {
                                self.retry_probe();
                            }
                        }
                    }
                });
            });
        });

        if let Some(action) = action {
            self.handle_menu_action(action);
        }
    }

    /// Read the result of an in-flight probe (if any). Resolves the
    /// connection state and, when transitioning to Online, kicks off a
    /// cloud project list refresh so the Welcome screen has data to show.
    fn poll_probe_result(&mut self) {
        if self.probe_rx.is_none() {
            return;
        }
        let rx = self.probe_rx.as_ref().expect("probe_rx checked Some");
        match rx.try_recv() {
            Ok(result) => {
                let was_online = self.connection.is_online();
                self.connection.mode = result.mode;
                self.connection.last_error = result.error;
                self.connection.probed_endpoint = result.endpoint;
                self.probe_rx = None;
                log::info!(
                    "hfrog: probe resolved → {:?} (endpoint={}, err='{}')",
                    self.connection.mode,
                    self.connection.probed_endpoint,
                    self.connection.last_error
                );
                // Auto-refresh the cloud project list on the leading edge of
                // going online, so Welcome is populated by the time the user
                // actually looks at it. Going offline drops the cached list
                // so stale entries can't leak through.
                if self.connection.is_online() && !was_online {
                    self.refresh_cloud_projects();
                } else if !self.connection.is_online() {
                    self.cloud_projects.clear();
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                // Worker thread died unexpectedly — collapse to Offline.
                self.connection.mode = crate::connection::ConnectionMode::Offline;
                self.connection.last_error =
                    "probe worker disconnected before sending result".to_string();
                self.probe_rx = None;
            }
        }
    }

    /// Read the result of an in-flight cloud-projects list call.
    fn poll_cloud_list_result(&mut self) {
        if self.cloud_list_rx.is_none() {
            return;
        }
        let rx = self
            .cloud_list_rx
            .as_ref()
            .expect("cloud_list_rx checked Some");
        match rx.try_recv() {
            Ok(Ok(projects)) => {
                log::info!("hfrog: cloud list refreshed — {} project(s)", projects.len());
                self.cloud_projects = projects;
                self.cloud_list_rx = None;
            }
            Ok(Err(err)) => {
                log::warn!("hfrog: cloud list refresh failed: {}", err);
                self.cloud_projects.clear();
                self.cloud_list_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.cloud_list_rx = None;
            }
        }
    }

    /// Manually re-probe hfrog. Bound to the menubar's "Try cloud" button
    /// when offline, and the Welcome screen's refresh button.
    fn retry_probe(&mut self) {
        // Only one probe in flight at a time — second click while probing
        // is a no-op rather than spawning a duplicate worker.
        if self.probe_rx.is_some() {
            return;
        }
        self.connection.mode = crate::connection::ConnectionMode::Probing;
        self.connection.last_error.clear();
        self.probe_rx = Some(crate::connection::spawn_probe(&self.config.hfrog));
    }

    /// Spawn a worker that fetches the cloud project list. Result lands in
    /// `self.cloud_projects` once `poll_cloud_list_result` reads the rx.
    fn refresh_cloud_projects(&mut self) {
        if self.cloud_list_rx.is_some() {
            return;
        }
        let cfg = self.config.hfrog.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::hfrog::list_cloud_projects(&cfg)
                .map_err(|e| format!("{}", e));
            let _ = tx.send(result);
        });
        self.cloud_list_rx = Some(rx);
    }

    /// Process global keyboard shortcuts. Called once per frame BEFORE the
    /// menu bar / panels are laid out so a shortcut firing on a given frame
    /// doesn't race with the menu rendering.
    ///
    /// Modifiers::COMMAND maps to ⌘ on macOS and Ctrl elsewhere — egui's
    /// cross-platform alias, so we don't have to special-case the OS.
    /// `consume_shortcut()` removes the keypress from egui's input queue,
    /// preventing downstream widgets (text fields, sliders) from also
    /// reacting to the same combo.
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Esc — dismiss the active dialog (toast list keeps draining itself).
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.open_dialog.is_some() {
            self.open_dialog = None;
        }

        // Quick helpers so the table below is one-liners.
        macro_rules! shortcut {
            ($mods:expr, $key:expr) => {
                ctx.input_mut(|i| {
                    i.consume_shortcut(&egui::KeyboardShortcut::new($mods, $key))
                })
            };
        }
        let cmd = egui::Modifiers::COMMAND;
        let cmd_shift = egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT);

        // When a text field has keyboard focus, defer undo / redo to its
        // own in-field history — that's what the user expects while typing
        // (matches every text editor / browser). File ops below still fire
        // regardless of focus, also matching editor conventions.
        let editing_text = ctx.wants_keyboard_input();

        if !editing_text {
            // Cmd+Y is the Windows-style alias for redo (keep both bindings).
            if shortcut!(cmd, egui::Key::Z) {
                self.undo_packer();
                return;
            }
            if shortcut!(cmd_shift, egui::Key::Z) || shortcut!(cmd, egui::Key::Y) {
                self.redo_packer();
                return;
            }
        }

        // File ops — these all funnel through `handle_menu_action` so
        // dirty-check dialogs / dispatch logic match menu-driven invocations
        // exactly. `Cmd+S` saves; `Cmd+Shift+S` triggers Save As.
        if shortcut!(cmd, egui::Key::N) {
            self.handle_menu_action(MenuAction::NewProject);
            return;
        }
        if shortcut!(cmd, egui::Key::O) {
            self.handle_menu_action(MenuAction::OpenProject);
            return;
        }
        if shortcut!(cmd_shift, egui::Key::S) {
            self.handle_menu_action(MenuAction::SaveProjectAs);
            return;
        }
        if shortcut!(cmd, egui::Key::S) {
            self.handle_menu_action(MenuAction::SaveProject);
            return;
        }
        if shortcut!(cmd, egui::Key::E) {
            self.handle_menu_action(MenuAction::ExportAs);
            return;
        }
        if shortcut!(cmd, egui::Key::W) {
            // Close current project / atlas — return to Welcome. Mirrors
            // typical "close window" semantics in document editors.
            if let AppMode::Packer(s) = &self.mode {
                if s.dirty {
                    self.open_dialog =
                        Some(Dialog::UnsavedChanges(Box::new(MenuAction::GoHome)));
                    return;
                }
            }
            self.handle_menu_action(MenuAction::GoHome);
            return;
        }
        if shortcut!(cmd, egui::Key::P) {
            // Manual pack — bypasses the auto-pack toggle, same as clicking
            // the Pack! button. Useful when the user has auto-pack disabled
            // and wants a deterministic "pack now".
            if let AppMode::Packer(state) = &mut self.mode {
                if !state.project.sprites.is_empty() && state.pack_rx.is_none() {
                    state.manual_pack = true;
                }
            }
        }
    }

    /// Pop one entry off the history stack and restore the previous Project.
    /// Drops the inline preview because the displayed atlas may no longer
    /// reflect the restored sprite list / settings — the next auto-pack (or
    /// Cmd+P) will regenerate it.
    fn undo_packer(&mut self) {
        if let AppMode::Packer(state) = &mut self.mode {
            if let Some(prev) = state.history.undo() {
                state.project = prev;
                state.dirty = true;
                state.preview = None;
                state.needs_auto_pack = true;
                self.toast("Undo", ToastKind::Info);
            } else {
                self.toast("Nothing to undo", ToastKind::Info);
            }
        }
    }

    fn redo_packer(&mut self) {
        if let AppMode::Packer(state) = &mut self.mode {
            if let Some(next) = state.history.redo() {
                state.project = next;
                state.dirty = true;
                state.preview = None;
                state.needs_auto_pack = true;
                self.toast("Redo", ToastKind::Info);
            } else {
                self.toast("Nothing to redo", ToastKind::Info);
            }
        }
    }

    fn handle_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::NewProject => {
                if self.is_packer_dirty() {
                    self.open_dialog = Some(Dialog::UnsavedChanges(Box::new(MenuAction::NewProject)));
                } else {
                    self.mode = AppMode::Packer(PackerState::new_empty());
                    self.toast("New project created", ToastKind::Info);
                }
            }
            MenuAction::OpenProject => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("mj_atlas project", &["tpproj"])
                    .pick_file()
                {
                    match Project::load(&path) {
                        Ok(project) => {
                            let count = project.sprites.len();
                            self.add_recent(&path);
                            self.mode = AppMode::Packer(PackerState::from_project(project, path));
                            self.toast(format!("Opened project ({} sprites)", count), ToastKind::Success);
                        }
                        Err(e) => {
                            self.toast(format!("Failed to open project: {}", e), ToastKind::Error);
                        }
                    }
                }
            }
            MenuAction::SaveProject => {
                self.save_project(false);
            }
            MenuAction::SaveProjectAs => {
                self.save_project(true);
            }
            MenuAction::AddSprites => {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"])
                    .pick_files()
                {
                    self.add_sprites_from_paths(&files);
                }
            }
            MenuAction::OpenAtlas => {
                self.open_atlas_dialog();
            }
            MenuAction::OpenRecent(path) => {
                // Try as project first, then as atlas
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if ext == "tpproj" {
                    match Project::load(&path) {
                        Ok(project) => {
                            let count = project.sprites.len();
                            self.add_recent(&path);
                            self.mode = AppMode::Packer(PackerState::from_project(project, path));
                            self.toast(format!("Opened project ({} sprites)", count), ToastKind::Success);
                        }
                        Err(e) => self.toast(format!("Failed: {}", e), ToastKind::Error),
                    }
                } else {
                    match load_atlas_for_viewer(&path) {
                        Ok(viewer) => {
                            self.add_recent(&path);
                            self.mode = AppMode::Viewer(viewer);
                        }
                        Err(e) => self.toast(format!("Failed: {}", e), ToastKind::Error),
                    }
                }
            }
            MenuAction::ExportAs => {
                self.open_dialog = Some(Dialog::ExportAs);
            }
            MenuAction::GoHome => {
                self.mode = AppMode::Welcome;
            }
            MenuAction::ShowAbout => {
                self.open_dialog = Some(Dialog::About);
            }
            MenuAction::ShowLicense => {
                self.open_dialog = Some(Dialog::License);
            }
            MenuAction::Undo => self.undo_packer(),
            MenuAction::Redo => self.redo_packer(),
            MenuAction::PackNow => {
                if let AppMode::Packer(state) = &mut self.mode {
                    if !state.project.sprites.is_empty() && state.pack_rx.is_none() {
                        state.manual_pack = true;
                    }
                }
            }
        }
    }

    fn is_packer_dirty(&self) -> bool {
        matches!(&self.mode, AppMode::Packer(s) if s.dirty)
    }

    fn save_project(&mut self, save_as: bool) {
        if let AppMode::Packer(state) = &mut self.mode {
            let path = if save_as || state.project_path.is_none() {
                rfd::FileDialog::new()
                    .add_filter("mj_atlas project", &["tpproj"])
                    .set_file_name(&format!("{}.tpproj", state.project.output_name))
                    .save_file()
            } else {
                state.project_path.clone()
            };

            if let Some(path) = path {
                match state.project.save(&path) {
                    Ok(()) => {
                        state.project_path = Some(path.clone());
                        state.dirty = false;
                        self.add_recent(&path);
                        self.toasts.push(Toast::new(
                            format!("Saved: {}", path.display()),
                            ToastKind::Success,
                        ));

                        // Best-effort hfrog mirror — runs synchronously here
                        // because save() already wrote to disk, so even if
                        // the upload hangs/fails the user's data is safe.
                        // The toast above already confirmed local success;
                        // we don't add a second toast on mirror failure (the
                        // runlog captures details for `_ai/troubleshooting`).
                        let cfg = self.config.clone();
                        if cfg.hfrog.is_active() {
                            let project_name = path
                                .file_stem()
                                .and_then(|s| s.to_str())
                                .unwrap_or("project")
                                .to_string();
                            let ver = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map(|d| format!("save-{}", d.as_secs()))
                                .unwrap_or_else(|_| "save-0".to_string());
                            crate::hfrog::mirror_paths(
                                &cfg.hfrog,
                                &project_name,
                                &ver,
                                &[(path.clone(), "tpproj")],
                            );
                        }
                    }
                    Err(e) => {
                        self.toasts.push(Toast::new(
                            format!("Save failed: {}", e),
                            ToastKind::Error,
                        ));
                    }
                }
            }
        }
    }

    fn add_sprites_from_paths(&mut self, paths: &[PathBuf]) {
        let image_exts = ["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"];
        if let AppMode::Packer(state) = &mut self.mode {
            let mut added = 0;
            for path in paths {
                if path.is_dir() {
                    // Recursively scan directories
                    for entry in walkdir::WalkDir::new(path).into_iter().flatten() {
                        if entry.file_type().is_file() {
                            let ext = entry.path().extension()
                                .and_then(|e| e.to_str())
                                .map(|e| e.to_lowercase())
                                .unwrap_or_default();
                            if image_exts.contains(&ext.as_str()) {
                                let ps = entry.path().display().to_string();
                                if !state.project.sprites.contains(&ps) {
                                    state.project.sprites.push(ps);
                                    added += 1;
                                }
                            }
                        }
                    }
                } else {
                    let ext = path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase())
                        .unwrap_or_default();
                    if image_exts.contains(&ext.as_str()) {
                        let path_str = path.display().to_string();
                        if !state.project.sprites.contains(&path_str) {
                            state.project.sprites.push(path_str);
                            added += 1;
                        }
                    }
                }
            }
            if added > 0 {
                state.dirty = true;
                state.needs_auto_pack = true;
                state.status = PackStatus::Success(format!("Added {} sprite(s)", added));
            }
        }
    }

    // ─── Toast overlay ───

    fn ui_toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| !t.is_expired());

        if self.toasts.is_empty() {
            return;
        }

        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_TOP, [-16.0, 40.0])
            .show(ctx, |ui| {
                for toast in &self.toasts {
                    let elapsed = toast.created.elapsed().as_secs_f32();
                    let alpha = if elapsed > 4.0 {
                        ((5.0 - elapsed) * 255.0) as u8
                    } else {
                        255
                    };

                    let bg = toast.color();
                    let bg = egui::Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), alpha);

                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(6.0)
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&toast.message)
                                    .color(egui::Color32::from_rgba_unmultiplied(255, 255, 255, alpha))
                                    .size(13.0),
                            );
                        });
                    ui.add_space(4.0);
                }
            });

        // Request repaint for fade animation
        ctx.request_repaint();
    }

    // ─── Dialogs ───

    fn ui_dialogs(&mut self, ctx: &egui::Context) {
        let mut close_dialog = false;
        let mut deferred_action: Option<DeferredDialogAction> = None;

        if let Some(dialog) = &self.open_dialog {
            match dialog {
                Dialog::About => {
                    egui::Window::new("About mj_atlas")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(8.0);
                                ui.heading(
                                    egui::RichText::new("mj_atlas")
                                        .size(28.0)
                                        .strong(),
                                );
                                ui.add_space(4.0);
                                ui.label(format!("Version {}", VERSION));
                                ui.add_space(8.0);
                                ui.label("Game-ready texture atlas packer");
                                ui.label("Polygon mesh / Trim / Extrude / Dedup / Quantize");
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(4.0);
                                ui.label("Render backend: wgpu (Metal / Vulkan / DX12)");
                                ui.label("Packing engine: crunch (tree-split bin packer)");
                                ui.label("Triangulation: earcut");
                                ui.add_space(8.0);
                                ui.label(
                                    egui::RichText::new("Open source — MIT License")
                                        .color(egui::Color32::GRAY),
                                );
                                ui.add_space(8.0);
                                if ui.button("OK").clicked() {
                                    close_dialog = true;
                                }
                            });
                        });
                }
                Dialog::License => {
                    egui::Window::new("License")
                        .collapsible(false)
                        .resizable(true)
                        .default_size([500.0, 300.0])
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                ui.label(egui::RichText::new("mj_atlas").strong().size(16.0));
                                ui.add_space(4.0);
                                ui.label(format!("Version: {}", VERSION));
                                ui.label("License: MIT");
                                ui.add_space(8.0);
                                ui.label("This software is free and open source.");
                                ui.label("You may use it for any purpose, including commercial projects.");
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("Third-party licenses:").strong());
                                ui.add_space(4.0);
                                ui.label("crunch 0.5 — MIT");
                                ui.label("earcut 0.4 — ISC");
                                ui.label("image 0.25 — MIT");
                                ui.label("egui / eframe 0.31 — MIT/Apache-2.0");
                                ui.label("imagequant 4 — GPL-3.0 (quantize feature)");
                                ui.label("lodepng 3 — Zlib");
                                ui.add_space(8.0);
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 180, 50),
                                    "Note: PNG quantize uses imagequant (GPL-3.0).\n\
                                     If distributing a binary with --quantize, the binary \n\
                                     is subject to GPL-3.0 terms.",
                                );
                                ui.add_space(8.0);
                            });
                            if ui.button("Close").clicked() {
                                close_dialog = true;
                            }
                        });
                }
                Dialog::UnsavedChanges(next_action) => {
                    let next = next_action.clone();
                    egui::Window::new("Unsaved Changes")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label("Current project has unsaved changes.");
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                if ui.button("Save & Continue").clicked() {
                                    deferred_action = Some(DeferredDialogAction::SaveThenDo(*next.clone()));
                                    close_dialog = true;
                                }
                                if ui.button("Discard").clicked() {
                                    deferred_action = Some(DeferredDialogAction::Do(*next));
                                    close_dialog = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    close_dialog = true;
                                }
                            });
                        });
                }
                Dialog::ExportAs => {
                    egui::Window::new("Export As...")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label("Export current atlas to a different format:");
                            ui.add_space(8.0);

                            if ui.button("  JSON Hash (.json)  ").clicked() {
                                self.export_current_atlas("json");
                                close_dialog = true;
                            }
                            ui.add_space(4.0);
                            if ui.button("  JSON Array (.json)  ").clicked() {
                                self.export_current_atlas("json-array");
                                close_dialog = true;
                            }
                            ui.add_space(4.0);
                            if ui.button("  Godot .tpsheet  ").clicked() {
                                self.export_current_atlas("tpsheet");
                                close_dialog = true;
                            }
                            ui.add_space(8.0);
                            if ui.button("Cancel").clicked() {
                                close_dialog = true;
                            }
                        });
                }
            }
        }

        if close_dialog {
            self.open_dialog = None;
        }

        // Process deferred actions from dialogs
        if let Some(da) = deferred_action {
            match da {
                DeferredDialogAction::SaveThenDo(action) => {
                    self.save_project(false);
                    self.handle_menu_action(action);
                }
                DeferredDialogAction::Do(action) => {
                    // Force: clear dirty so action proceeds
                    if let AppMode::Packer(s) = &mut self.mode {
                        s.dirty = false;
                    }
                    self.handle_menu_action(action);
                }
            }
        }
    }

    fn export_current_atlas(&mut self, format: &str) {
        let ext = match format {
            "tpsheet" => "tpsheet",
            _ => "json",
        };

        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Output", &[ext])
            .save_file()
        {
            // Snapshot the source path so we can release the &self.mode
            // borrow before reaching for self.config / self.toast.
            let source_path = match &self.mode {
                AppMode::Viewer(state) => Some(PathBuf::from(&state.source_path)),
                _ => None,
            };
            let Some(source) = source_path else {
                return;
            };
            let content = match std::fs::read_to_string(&source) {
                Ok(c) => c,
                Err(e) => {
                    self.toast(format!("Export read failed: {}", e), ToastKind::Error);
                    return;
                }
            };
            if let Err(e) = std::fs::write(&path, &content) {
                self.toast(format!("Export failed: {}", e), ToastKind::Error);
                return;
            }
            self.toast(
                format!("Exported to {}", path.display()),
                ToastKind::Success,
            );

            // Best-effort hfrog mirror — same pattern as save_project.
            let cfg = self.config.clone();
            if cfg.hfrog.is_active() {
                let project_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("export")
                    .to_string();
                let ver = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| format!("export-{}", d.as_secs()))
                    .unwrap_or_else(|_| "export-0".to_string());
                let kind = match format {
                    "tpsheet" => "atlas-tpsheet",
                    "json-array" => "atlas-json-array",
                    _ => "atlas-json",
                };
                crate::hfrog::mirror_paths(&cfg.hfrog, &project_name, &ver, &[(path.clone(), kind)]);
            }
        }
    }

    // ─── Welcome screen ──

    fn ui_welcome(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading(egui::RichText::new("mj_atlas").size(40.0).strong());
                ui.add_space(4.0);
                ui.label(egui::RichText::new("Game-ready Texture Atlas Packer").size(16.0).color(egui::Color32::GRAY));
                ui.label(egui::RichText::new(format!("v{}", VERSION)).size(12.0).color(egui::Color32::DARK_GRAY));
                ui.add_space(28.0);

                let btn_size = egui::vec2(300.0, 40.0);

                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new("New Project").size(15.0))).clicked() {
                    self.mode = AppMode::Packer(PackerState::new_empty());
                }
                ui.add_space(6.0);
                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new("Open Project...").size(15.0))).clicked() {
                    self.handle_menu_action(MenuAction::OpenProject);
                }
                ui.add_space(6.0);
                if ui.add_sized(btn_size, egui::Button::new(egui::RichText::new("Open Atlas Preview...").size(15.0))).clicked() {
                    self.open_atlas_dialog();
                }

                // ── Merged project list (cloud + local) ──
                //
                // Each row carries an origin badge:
                //   ☁︎  = only on hfrog
                //   💾 = only on disk (recent_files)
                //   ☁︎💾 = both (display name matches a cloud project AND a
                //          local recent path). Clicking a synced row prefers
                //          local — it's instant and the cloud copy is
                //          identical bytes.
                //
                // Connection status decides what we can show:
                //   Probing  → spinner placeholder
                //   Online   → cloud + local merged
                //   Offline  → local-only with a hint to retry the probe
                self.draw_welcome_project_list(ui);
            });
        });
    }

    /// Render the merged project picker on the Welcome screen. Extracted from
    /// `ui_welcome` so the logic stays readable as the merge rules grow.
    fn draw_welcome_project_list(&mut self, ui: &mut egui::Ui) {
        use crate::connection::ConnectionMode;

        ui.add_space(20.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Projects").size(13.0).color(egui::Color32::GRAY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Refresh re-probes hfrog AND re-fetches the cloud list.
                if ui
                    .small_button("Refresh")
                    .on_hover_text("Re-probe hfrog and refresh project list")
                    .clicked()
                {
                    self.retry_probe();
                    if self.connection.is_online() {
                        self.refresh_cloud_projects();
                    }
                }
            });
        });
        ui.add_space(4.0);

        // Build a unified row list with origin tags. Use a BTreeMap keyed by
        // display name so duplicate-named projects from cloud + local merge
        // automatically into one row.
        #[derive(Default)]
        struct Origins {
            cloud: Option<crate::hfrog::CloudProject>,
            local: Option<PathBuf>,
        }
        let mut rows: std::collections::BTreeMap<String, Origins> = Default::default();
        for cp in &self.cloud_projects {
            rows.entry(cp.display_name.clone())
                .or_default()
                .cloud = Some(cp.clone());
        }
        for path in &self.recent_files {
            let display = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            rows.entry(display).or_default().local = Some(path.clone());
        }

        // Probing placeholder so the user doesn't think "empty list = bug".
        if matches!(self.connection.mode, ConnectionMode::Probing) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    egui::RichText::new("Checking hfrog…")
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                );
            });
        }

        if rows.is_empty()
            && !matches!(self.connection.mode, ConnectionMode::Probing)
        {
            ui.label(
                egui::RichText::new("(no projects yet — click New Project)")
                    .size(12.0)
                    .color(egui::Color32::DARK_GRAY),
            );
        }

        let mut open_local: Option<PathBuf> = None;
        let mut open_cloud: Option<crate::hfrog::CloudProject> = None;
        for (display, origins) in &rows {
            let badge = match (&origins.cloud, &origins.local) {
                (Some(_), Some(_)) => "☁︎💾",
                (Some(_), None) => "☁︎",
                (None, Some(_)) => "💾",
                (None, None) => continue,
            };
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(badge)
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                );
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(display)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(100, 180, 255)),
                    )
                    .sense(egui::Sense::click()),
                );
                if resp.clicked() {
                    // Prefer local on synced rows (instant + identical bytes).
                    if let Some(p) = &origins.local {
                        open_local = Some(p.clone());
                    } else if let Some(cp) = &origins.cloud {
                        open_cloud = Some(cp.clone());
                    }
                }
                if let Some(cp) = &origins.cloud {
                    ui.label(
                        egui::RichText::new(format!("v{}", short_ver(&cp.ver)))
                            .size(11.0)
                            .color(egui::Color32::DARK_GRAY),
                    );
                }
            });
        }

        if let Some(path) = open_local {
            self.handle_menu_action(MenuAction::OpenRecent(path));
        }
        if let Some(cp) = open_cloud {
            self.open_cloud_project(&cp);
        }

        if matches!(self.connection.mode, ConnectionMode::Offline)
            && !self.connection.last_error.is_empty()
        {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "⚠ Offline mode — local-only ({})",
                    self.connection.last_error
                ))
                .size(11.0)
                .color(egui::Color32::DARK_GRAY),
            );
        }
    }

    /// Download a `.tpproj` from hfrog and load it into a fresh PackerState.
    /// Best-effort: failure surfaces as a toast and we stay on Welcome.
    fn open_cloud_project(&mut self, cp: &crate::hfrog::CloudProject) {
        let cfg = self.config.hfrog.clone();
        let cp_owned = cp.clone();
        // Synchronous download — small file, no need for a background thread
        // unless the user complains about UI lag here. Wrap in a thread later
        // if the hfrog round-trip starts feeling sticky.
        let bytes = match crate::hfrog::download_project(&cfg, &cp_owned) {
            Ok(b) => b,
            Err(e) => {
                self.toast(format!("Cloud open failed: {}", e), ToastKind::Error);
                return;
            }
        };
        let parsed: std::result::Result<Project, String> = serde_json::from_slice(&bytes)
            .map_err(|e| format!("parse tpproj: {}", e));
        let project = match parsed {
            Ok(p) => p,
            Err(e) => {
                self.toast(format!("Cloud project parse failed: {}", e), ToastKind::Error);
                return;
            }
        };
        // No persistent disk path — cloud-loaded projects are "untitled" until
        // the user does Save As. They get the dirty marker so Save prompts.
        let mut state = PackerState::from_project(project, PathBuf::from(&cp_owned.display_name));
        state.project_path = None;
        state.dirty = true;
        self.mode = AppMode::Packer(state);
        self.toast(
            format!("Loaded '{}' from hfrog", cp_owned.display_name),
            ToastKind::Success,
        );
    }
}

/// Truncate a long version string so it fits in the project list. SHA-prefix
/// versions look ugly past 8 chars in a tight UI; for "save-<unix>" timestamps
/// we just show the trailing suffix.
fn short_ver(v: &str) -> String {
    if v.is_empty() {
        return "?".to_string();
    }
    if v.len() <= 12 {
        v.to_string()
    } else {
        format!("{}…", &v[..10])
    }
}

impl MJAtlasApp {

    // ─── Packer configuration screen ──

    /// Launch a background pack for the current project state.
    fn trigger_pack(&mut self, ctx: &egui::Context) {
        if let AppMode::Packer(state) = &mut self.mode {
            if state.pack_rx.is_some() || state.project.sprites.is_empty() {
                return; // Already packing or nothing to pack
            }

            let s = &state.project.settings;
            let out_dir = if state.project.output_dir.is_empty() {
                state.project_path.as_ref()
                    .and_then(|p| p.parent())
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| std::env::temp_dir().display().to_string())
            } else {
                state.project.output_dir.clone()
            };

            let input_dir = find_common_parent(&state.project.sprites);

            // Pack EXACTLY the user's curated list — never the directory at
            // large. Otherwise the GUI list and the preview diverge as soon
            // as `find_common_parent` lands on a folder containing files the
            // user didn't pick (and deletions silently get re-added on the
            // next scan). Both bugs fixed by routing through explicit_sprites.
            let explicit: Vec<PathBuf> = state
                .project
                .sprites
                .iter()
                .map(PathBuf::from)
                .collect();

            let opts = PackOptions {
                input_dir: PathBuf::from(&input_dir),
                output_name: state.project.output_name.clone(),
                output_dir: PathBuf::from(&out_dir),
                max_size: s.max_size,
                spacing: s.spacing,
                padding: s.padding,
                extrude: s.extrude,
                trim: s.trim,
                trim_threshold: s.trim_threshold,
                rotate: s.rotate,
                pot: s.pot,
                recursive: true,
                explicit_sprites: Some(explicit),
                incremental: false,
                force: false,
                format: output::Format::JsonHash,
                quantize: s.quantize,
                quantize_quality: s.quantize_quality,
                polygon: s.polygon,
                tolerance: s.tolerance,
                polygon_shape: pack::PolygonShape::Concave,
                max_vertices: 0,
            };

            let (tx, rx) = std::sync::mpsc::channel();
            let ctx_clone = ctx.clone();
            // The log path must be derivable from `opts` BEFORE the thread
            // moves it. We capture it once so a failed pack still leaves a
            // sidecar the user can inspect from the GUI's working dir.
            let log_path = opts
                .output_dir
                .join(format!("{}.log", opts.output_name));
            std::thread::spawn(move || {
                // Header captures the GUI-resolved options so the sidecar
                // is self-contained when the user shares it for debugging.
                let mut header = crate::runlog::standard_header();
                header.push("subcommand: gui pack".to_string());
                header.push(format!("input_dir:  {}", opts.input_dir.display()));
                header.push(format!(
                    "output:     {}/{}.png",
                    opts.output_dir.display(),
                    opts.output_name
                ));
                header.push(format!(
                    "explicit_sprites: {}",
                    opts.explicit_sprites
                        .as_ref()
                        .map(|v| v.len().to_string())
                        .unwrap_or_else(|| "(none — directory scan)".into())
                ));
                header.push(format!(
                    "layout: max_size={} spacing={} padding={} extrude={} trim={} rotate={} pot={}",
                    opts.max_size, opts.spacing, opts.padding, opts.extrude,
                    opts.trim, opts.rotate, opts.pot
                ));

                let result = match pack::execute(&opts) {
                    Ok(results) => {
                        let total: usize = results.iter().map(|r| r.sprites.len()).sum();
                        let msg = format!("Packed {} atlas(es), {} sprites (preview only — use File > Export to save)",
                            results.len(), total);
                        // Take atlas image directly from memory — zero IO
                        if let Some(first) = results.into_iter().next() {
                            let (aw, ah) = (first.width, first.height);
                            let sprites: Vec<SpriteInfo> = first.sprites.iter().map(|sp| {
                                SpriteInfo {
                                    name: sp.name.clone(),
                                    x: sp.x as f32, y: sp.y as f32,
                                    w: sp.w as f32, h: sp.h as f32,
                                    rotated: sp.rotated,
                                    source_w: sp.source_w as f32,
                                    source_h: sp.source_h as f32,
                                    mesh_vertices: sp.vertices_uv.clone(),
                                    mesh_triangles: sp.triangles.clone(),
                                }
                            }).collect();
                            BackgroundPackResult::Success {
                                message: msg,
                                atlas_image: first.atlas_image,
                                sprites, atlas_w: aw, atlas_h: ah,
                            }
                        } else {
                            BackgroundPackResult::Error("No atlas produced".into())
                        }
                    }
                    Err(e) => {
                        // Surface the error into the runlog buffer so the
                        // sidecar reflects the failure, not just the success
                        // path's INFO messages.
                        log::error!("Pack failed: {}", e);
                        BackgroundPackResult::Error(format!("Pack: {}", e))
                    }
                };
                // Always flush the log — a successful pack tells the user
                // what was packed; a failed one tells them what went wrong.
                crate::runlog::flush(&log_path, &header);
                let _ = tx.send(result);
                ctx_clone.request_repaint();
            });

            state.pack_rx = Some(rx);
            state.status = PackStatus::Success("Packing...".to_string());
        }
    }

    fn ui_packer(&mut self, ctx: &egui::Context) {
        // Handle file drops
        let dropped_files: Vec<PathBuf> = ctx.input(|i| {
            i.raw.dropped_files.iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !dropped_files.is_empty() {
            self.add_sprites_from_paths(&dropped_files);
        }

        // Detect drag hovering
        let is_dragging_over = ctx.input(|i| !i.raw.hovered_files.is_empty());

        // Poll background pack result
        if let AppMode::Packer(state) = &mut self.mode {
            if let Some(rx) = &state.pack_rx {
                if let Ok(result) = rx.try_recv() {
                    match result {
                        BackgroundPackResult::Success { message, atlas_image, sprites, atlas_w, atlas_h } => {
                            state.status = PackStatus::Success(message);
                            state.preview = Some(InlinePreview {
                                texture: None,
                                image: atlas_image,
                                sprites,
                                atlas_w, atlas_h,
                                zoom: 1.0,
                                pan_offset: egui::Vec2::ZERO,
                                hovered: None,
                                show_grid: true,
                                show_names: false,
                                show_mesh: false,
                                needs_fit: true,
                            });
                        }
                        BackgroundPackResult::Error(e) => {
                            state.status = PackStatus::Error(e);
                        }
                    }
                    state.pack_rx = None;
                }
            }
            if state.pack_rx.is_some() {
                ctx.request_repaint();
            }

        }

        // Trigger pack (must be outside the borrow above). Two gates:
        //   - `manual_pack` (Pack! button): always honored — explicit user
        //     intent must work even with auto-pack disabled.
        //   - `needs_auto_pack` (drop/add/delete/settings event): honored
        //     only when `auto_pack` toggle is on, so the toggle is not
        //     decorative.
        let should_run = matches!(
            &self.mode,
            AppMode::Packer(s)
                if s.pack_rx.is_none()
                && !s.project.sprites.is_empty()
                && (s.manual_pack || (s.needs_auto_pack && s.auto_pack))
        );
        if should_run {
            if let AppMode::Packer(state) = &mut self.mode {
                state.needs_auto_pack = false;
                state.manual_pack = false;
            }
            self.trigger_pack(ctx);
        }

        let is_packing_now = matches!(&self.mode, AppMode::Packer(s) if s.pack_rx.is_some());
        let list_changed = std::cell::Cell::new(false);

        // Deferred flags from the settings panel's hfrog "Save settings"
        // button. We can't call `self.toast()` from inside the panel closure
        // because `self.mode` is borrowed; we capture the result here and
        // surface a toast after the closure ends.
        let mut save_config_ok = false;
        let mut save_config_err: Option<String> = None;

        if let AppMode::Packer(state) = &mut self.mode {
            let is_packing = state.pack_rx.is_some();
            let s = &mut state.project.settings;

            // ── Status bar ──
            egui::TopBottomPanel::bottom("pack_status").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (color, msg) = match &state.status {
                        PackStatus::Idle => (egui::Color32::GRAY, format!("{} sprites in project", state.project.sprites.len())),
                        PackStatus::Success(m) => (egui::Color32::from_rgb(50, 200, 80), m.clone()),
                        PackStatus::Error(m) => (egui::Color32::from_rgb(220, 60, 60), m.clone()),
                    };
                    ui.colored_label(color, msg);
                });
            });

            // ── Right panel: settings ──
            //
            // min_width(280): the polygon-mesh tolerance row needs ~270 px to
            //   show its label + DragValue without truncation.
            // max_width(400): prevents the user from dragging the divider so
            //   far that the central canvas collapses to nothing.
            egui::SidePanel::right("settings_panel")
                .default_width(300.0)
                .min_width(280.0)
                .max_width(400.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // Output
                        ui.heading("Output");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            if ui.text_edit_singleline(&mut state.project.output_name).changed() {
                                state.dirty = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Dir:");
                            if ui.add_sized(
                                [(ui.available_width() - 56.0).max(60.0), 18.0],
                                egui::TextEdit::singleline(&mut state.project.output_dir),
                            ).changed() {
                                state.dirty = true;
                            }
                            if ui.button("...").clicked() {
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    state.project.output_dir = dir.display().to_string();
                                    state.dirty = true;
                                }
                            }
                        });
                        ui.add_space(8.0);
                        ui.separator();

                        // Packing
                        ui.heading("Packing");
                        ui.add_space(4.0);
                        // Each pack-affecting widget records both `dirty` (so
                        // the project gets a save prompt later) AND
                        // `needs_auto_pack` (so the preview catches up to the
                        // new options on the very next frame). Pre-v0.3.3 only
                        // a couple of widgets even tracked .changed(), so e.g.
                        // toggling "Power-of-2" silently kept showing the old
                        // atlas.
                        let mut pack_settings_changed: Option<&'static str> = None;
                        ui.horizontal(|ui| {
                            ui.label("Max size:");
                            egui::ComboBox::from_id_salt("max_size")
                                .selected_text(format!("{}", s.max_size))
                                .show_ui(ui, |ui| {
                                    for sz in [256, 512, 1024, 2048, 4096, 8192] {
                                        if ui
                                            .selectable_value(&mut s.max_size, sz, format!("{}", sz))
                                            .changed()
                                        {
                                            pack_settings_changed = Some("max_size");
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Spacing:");
                            if ui
                                .add(egui::DragValue::new(&mut s.spacing).range(0..=32))
                                .changed()
                            {
                                pack_settings_changed = Some("spacing");
                            }
                            ui.label("Pad:");
                            if ui
                                .add(egui::DragValue::new(&mut s.padding).range(0..=32))
                                .changed()
                            {
                                pack_settings_changed = Some("padding");
                            }
                            ui.label("Extr:");
                            if ui
                                .add(egui::DragValue::new(&mut s.extrude).range(0..=8))
                                .changed()
                            {
                                pack_settings_changed = Some("extrude");
                            }
                        });
                        if ui.checkbox(&mut s.trim, "Trim transparent").changed() {
                            pack_settings_changed = Some("trim");
                        }
                        if ui.checkbox(&mut s.rotate, "Allow rotation").changed() {
                            pack_settings_changed = Some("rotate");
                        }
                        if ui.checkbox(&mut s.pot, "Power-of-2").changed() {
                            pack_settings_changed = Some("pot");
                        }
                        if ui.checkbox(&mut s.polygon, "Polygon mesh").changed() {
                            pack_settings_changed = Some("polygon");
                        }
                        if s.polygon {
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.label("Tolerance:");
                                if ui
                                    .add(
                                        egui::DragValue::new(&mut s.tolerance)
                                            .range(0.5..=10.0)
                                            .speed(0.1),
                                    )
                                    .changed()
                                {
                                    pack_settings_changed = Some("tolerance");
                                }
                            });
                        }
                        // Quantize / quantize_quality only affect on-disk
                        // encoding (imagequant runs in save_to_disk, not in
                        // pack::execute). They're "dirty" but don't justify
                        // a fresh in-memory pack — the preview pixels are
                        // identical either way.
                        if ui.checkbox(&mut s.quantize, "PNG quantize").changed() {
                            state.dirty = true;
                        }
                        if s.quantize {
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.label("Quality:");
                                if ui
                                    .add(egui::DragValue::new(&mut s.quantize_quality).range(1..=100))
                                    .changed()
                                {
                                    state.dirty = true;
                                }
                            });
                        }

                        if let Some(field) = pack_settings_changed {
                            // Surface the change in the runlog so reviewing the
                            // sidecar shows which knob was twisted between two
                            // packs (essential when several settings move
                            // before the user clicks anywhere else).
                            log::info!("gui: settings changed — {} (queued auto-pack)", field);
                            state.dirty = true;
                            state.needs_auto_pack = true;
                        }

                        ui.add_space(8.0);
                        ui.separator();

                        // Format
                        ui.heading("Format");
                        for (i, (_, desc)) in FORMAT_NAMES.iter().enumerate() {
                            if ui.radio_value(&mut s.format_idx, i, *desc).changed() {
                                state.dirty = true;
                            }
                        }

                        ui.add_space(8.0);
                        ui.separator();

                        // Preview layout
                        ui.heading("Preview");
                        ui.horizontal(|ui| {
                            ui.label("Split:");
                            ui.radio_value(&mut state.split_dir, SplitDir::Horizontal, "Left|Right");
                            ui.radio_value(&mut state.split_dir, SplitDir::Vertical, "Top|Bottom");
                        });
                        ui.checkbox(&mut state.auto_pack, "Auto-pack on change");
                        if state.preview.is_some() {
                            if let Some(p) = &state.preview {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Atlas: {}x{}, {} sprites",
                                        p.atlas_w,
                                        p.atlas_h,
                                        p.sprites.len()
                                    ))
                                    .small()
                                    .monospace()
                                    .color(egui::Color32::GRAY),
                                );
                            }
                        }

                        ui.add_space(12.0);

                        // Pack button
                        let can_pack = !state.project.sprites.is_empty() && !is_packing;
                        let pack_label = if is_packing { "  Packing...  " } else { "  Pack!  " };
                        if ui
                            .add_enabled(
                                can_pack,
                                egui::Button::new(
                                    egui::RichText::new(pack_label).size(18.0).strong(),
                                ).min_size(egui::vec2(ui.available_width(), 36.0)),
                            )
                            .clicked()
                        {
                            // Manual click — always run, even if auto-pack
                            // is off (the toggle gates only event-driven
                            // packs, not explicit Pack! presses).
                            state.manual_pack = true;
                        }

                        if is_packing {
                            ui.spinner();
                        }

                        ui.add_space(12.0);
                        ui.separator();

                        // ── hfrog mirror section ──
                        // Editable in-place; persists to ~/.config/mj_atlas/
                        // config.toml on click of "Save settings". Empty
                        // endpoint = mirror disabled even when checkbox is on.
                        ui.heading("hfrog Mirror");
                        ui.add_space(4.0);
                        if ui
                            .checkbox(
                                &mut self.config.hfrog.enabled,
                                "Mirror to hfrog on Save / Export",
                            )
                            .changed()
                        {
                            self.config_dirty = true;
                        }
                        ui.horizontal(|ui| {
                            ui.label("Endpoint:");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.config.hfrog.endpoint)
                                        .hint_text("https://hfrog.example.com"),
                                )
                                .changed()
                            {
                                self.config_dirty = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.label("Token:   ");
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut self.config.hfrog.token)
                                        .password(true)
                                        .hint_text("Bearer token (optional)"),
                                )
                                .changed()
                            {
                                self.config_dirty = true;
                            }
                        });
                        if self.config_dirty {
                            ui.horizontal(|ui| {
                                if ui.button("Save settings").clicked() {
                                    match self.config.save() {
                                        Ok(()) => {
                                            self.config_dirty = false;
                                            save_config_ok = true;
                                        }
                                        Err(e) => {
                                            save_config_err = Some(format!("{}", e));
                                        }
                                    }
                                }
                                if ui.small_button("Reset").clicked() {
                                    if let Ok(loaded) = crate::config::Config::load() {
                                        self.config = loaded;
                                        self.config_dirty = false;
                                    }
                                }
                            });
                        }
                        let status_text = if self.config.hfrog.is_active() {
                            "● mirror active"
                        } else if self.config.hfrog.enabled {
                            "○ enabled but endpoint missing"
                        } else {
                            "○ mirror disabled"
                        };
                        ui.label(
                            egui::RichText::new(status_text)
                                .small()
                                .color(egui::Color32::GRAY),
                        );
                    });
                });

            // ── Central area: sprite list + preview ──
            let has_preview = state.preview.is_some();
            let split = state.split_dir;

            // When we have a preview in horizontal mode, use a left SidePanel for sprite list
            if has_preview && split == SplitDir::Horizontal {
                // max_width(400): same rationale as the settings panel — keep
                // the central canvas from being squeezed to a sliver.
                egui::SidePanel::left("sprite_list_panel")
                    .default_width(280.0)
                    .min_width(200.0)
                    .max_width(400.0)
                    .resizable(true)
                    .show(ctx, |ui| {
                        if draw_sprite_list(ui, &mut state.project, &mut state.search_text, &mut state.dirty, &mut state.thumbnails) {
                            list_changed.set(true);
                        }
                    });

                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(preview) = &mut state.preview {
                        draw_inline_preview(ui, preview);
                    }
                });
            } else if has_preview && split == SplitDir::Vertical {
                // max_height(60% of viewport) keeps the canvas at least 40%
                // of vertical space — same anti-collapse rationale as the
                // horizontal split's max_width.
                let vp = ctx.screen_rect().height();
                egui::TopBottomPanel::top("sprite_list_panel_v")
                    .default_height(200.0)
                    .min_height(100.0)
                    .max_height((vp * 0.6).max(200.0))
                    .resizable(true)
                    .show(ctx, |ui| {
                        if draw_sprite_list(ui, &mut state.project, &mut state.search_text, &mut state.dirty, &mut state.thumbnails) {
                            list_changed.set(true);
                        }
                    });

                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(preview) = &mut state.preview {
                        draw_inline_preview(ui, preview);
                    }
                });
            } else {
                // No preview — full central panel
                egui::CentralPanel::default().show(ctx, |ui| {
                    if state.project.sprites.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() / 3.0);
                            ui.heading(egui::RichText::new("Drop sprite images here").size(22.0).color(egui::Color32::GRAY));
                            ui.add_space(8.0);
                            ui.label(egui::RichText::new("or File > Add Sprites").size(14.0).color(egui::Color32::DARK_GRAY));
                            ui.add_space(16.0);
                            if ui.button(egui::RichText::new("  Add Sprites...  ").size(15.0)).clicked() {
                                if let Some(files) = rfd::FileDialog::new()
                                    .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"])
                                    .pick_files()
                                {
                                    for path in &files {
                                        let ps = path.display().to_string();
                                        if !state.project.sprites.contains(&ps) {
                                            state.project.sprites.push(ps);
                                        }
                                    }
                                    state.dirty = true;
                                }
                            }
                            if ui.button(egui::RichText::new("  Add Folder...  ").size(15.0)).clicked() {
                                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                    let image_exts = ["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"];
                                    for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                                        if entry.file_type().is_file() {
                                            let ext = entry.path().extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
                                            if image_exts.contains(&ext.as_str()) {
                                                let ps = entry.path().display().to_string();
                                                if !state.project.sprites.contains(&ps) {
                                                    state.project.sprites.push(ps);
                                                }
                                            }
                                        }
                                    }
                                    state.dirty = true;
                                }
                            }
                        });
                    } else {
                        if draw_sprite_list(ui, &mut state.project, &mut state.search_text, &mut state.dirty, &mut state.thumbnails) {
                            list_changed.set(true);
                        }
                    }
                });
            }
        }

        // ── React to sprite list changes ──
        if list_changed.get() {
            if let AppMode::Packer(state) = &mut self.mode {
                if state.project.sprites.is_empty() {
                    // Cleared all — remove preview
                    state.preview = None;
                } else {
                    // List changed — re-pack
                    state.needs_auto_pack = true;
                }
            }
        }

        // ── React to config save attempts (deferred from settings panel) ──
        if save_config_ok {
            self.toast("Settings saved", ToastKind::Success);
        }
        if let Some(e) = save_config_err {
            self.toast(format!("Settings save failed: {}", e), ToastKind::Error);
        }

        // ── Drag hover overlay ──
        if is_dragging_over {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("drop_overlay"))
                .fixed_pos(screen.min)
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect, 0.0,
                        egui::Color32::from_rgba_unmultiplied(56, 132, 244, 40),
                    );
                    ui.painter().rect_stroke(
                        rect.shrink(4.0), 8.0,
                        egui::Stroke::new(3.0, egui::Color32::from_rgb(56, 132, 244)),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Drop sprites here",
                        egui::FontId::proportional(28.0),
                        egui::Color32::from_rgb(56, 132, 244),
                    );
                });
        }

        // ── Packing loading overlay ──
        if is_packing_now {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("packing_overlay"))
                .fixed_pos(screen.min)
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect, 0.0,
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 120),
                    );
                    ui.put(
                        egui::Rect::from_center_size(rect.center() - egui::vec2(0.0, 16.0), egui::vec2(40.0, 40.0)),
                        egui::Spinner::new().size(32.0),
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 24.0),
                        egui::Align2::CENTER_CENTER,
                        "Packing...",
                        egui::FontId::proportional(18.0),
                        egui::Color32::WHITE,
                    );
                });
        }
    }

    // ─── Viewer / Preview screen ──

    fn ui_viewer(&mut self, ctx: &egui::Context) {
        if let AppMode::Viewer(state) = &mut self.mode {
            // Ensure texture
            if state.atlas_texture.is_none() {
                let size = [state.atlas_w as usize, state.atlas_h as usize];
                let pixels: Vec<egui::Color32> = state
                    .atlas_image
                    .pixels()
                    .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
                    .collect();
                state.atlas_texture = Some(ctx.load_texture(
                    "atlas",
                    egui::ColorImage { size, pixels },
                    egui::TextureOptions::NEAREST,
                ));
            }

            // ── Left panel ──
            // Same min/max strategy as the packer's left panel: clamp the
            // resizable handle so dragging can't make either side disappear.
            egui::SidePanel::left("sprite_list")
                .default_width(260.0)
                .min_width(200.0)
                .max_width(400.0)
                .resizable(true)
                .show(ctx, |ui| {
                    ui.heading(format!(
                        "{} ({}x{})",
                        state.atlas_name, state.atlas_w, state.atlas_h
                    ));
                    ui.small(&state.source_path);
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("Search:");
                        ui.add_sized(
                            [ui.available_width(), 18.0],
                            egui::TextEdit::singleline(&mut state.search_text),
                        );
                    });
                    ui.separator();

                    if !state.animations.is_empty() {
                        ui.collapsing(
                            format!("Animations ({})", state.animations.len()),
                            |ui| {
                                for (name, frames) in &state.animations {
                                    ui.label(format!("  {} ({} frames)", name, frames.len()));
                                }
                            },
                        );
                        ui.separator();
                    }

                    ui.label(format!("{} sprites", state.sprites.len()));
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let search = state.search_text.to_lowercase();
                        for (i, sprite) in state.sprites.iter().enumerate() {
                            if !search.is_empty()
                                && !sprite.name.to_lowercase().contains(&search)
                            {
                                continue;
                            }
                            let is_selected = state.selected_sprite == Some(i);
                            // Monospace so wide names + dimensions stay
                            // column-aligned across rows. Easier to scan.
                            let label = egui::RichText::new(format!(
                                "{} ({}x{}{})",
                                sprite.name,
                                sprite.w as u32,
                                sprite.h as u32,
                                if sprite.rotated { " R" } else { "" }
                            ))
                            .monospace();
                            if ui.selectable_label(is_selected, label).clicked() {
                                state.selected_sprite = Some(i);
                                state.pan_offset = egui::vec2(
                                    -(sprite.x + sprite.w / 2.0),
                                    -(sprite.y + sprite.h / 2.0),
                                );
                            }
                        }
                    });
                });

            // ── Bottom controls ──
            //
            // Same stabilization pattern as the inline preview toolbar: always
            // allocate the hover info slot, otherwise TopBottomPanel auto-sizes
            // by content height and the central canvas above it shifts with
            // each hover transition.
            egui::TopBottomPanel::bottom("viewer_controls").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(24.0);
                    ui.label("Zoom:");
                    ui.add(egui::Slider::new(&mut state.zoom, 0.1..=10.0).logarithmic(true));
                    ui.checkbox(&mut state.show_grid, "Grid");
                    ui.checkbox(&mut state.show_names, "Names");
                    ui.checkbox(&mut state.show_mesh, "Mesh");
                    if ui.button("Fit").clicked() {
                        state.zoom = 1.0;
                        state.pan_offset = egui::Vec2::ZERO;
                        state.selected_sprite = None;
                    }

                    ui.separator();
                    let hover_text = state
                        .hovered_sprite
                        .and_then(|i| state.sprites.get(i))
                        .map(|s| {
                            format!(
                                "{} @ ({},{}) {}x{} src:{}x{}",
                                s.name,
                                s.x as u32,
                                s.y as u32,
                                s.w as u32,
                                s.h as u32,
                                s.source_w as u32,
                                s.source_h as u32
                            )
                        })
                        .unwrap_or_default();
                    // Coords and dimensions in monospace so they don't jiggle
                    // when the mouse moves between sprites of different sizes.
                    ui.label(egui::RichText::new(hover_text).monospace());
                });
            });

            // ── Central canvas ──
            egui::CentralPanel::default().show(ctx, |ui| {
                let (response, painter) =
                    ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());

                let canvas_center = response.rect.center();

                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 && response.hovered() {
                    state.zoom = (state.zoom * (scroll_delta * 0.005).exp()).clamp(0.05, 20.0);
                }

                if response.dragged_by(egui::PointerButton::Middle)
                    || response.dragged_by(egui::PointerButton::Secondary)
                    || (response.dragged_by(egui::PointerButton::Primary)
                        && ui.input(|i| i.modifiers.shift))
                {
                    state.pan_offset += response.drag_delta() / state.zoom;
                }

                let transform = |x: f32, y: f32| -> egui::Pos2 {
                    egui::pos2(
                        canvas_center.x + (x + state.pan_offset.x) * state.zoom,
                        canvas_center.y + (y + state.pan_offset.y) * state.zoom,
                    )
                };

                let tl = transform(0.0, 0.0);
                let br = transform(state.atlas_w as f32, state.atlas_h as f32);
                let atlas_rect = egui::Rect::from_min_max(tl, br);

                // Checkerboard
                let checker_size = 8.0 * state.zoom;
                let clip = response.rect.intersect(atlas_rect);
                if clip.is_positive() {
                    let dark = egui::Color32::from_gray(100);
                    let light = egui::Color32::from_gray(140);
                    let cols = ((clip.width() / checker_size).ceil() as i32 + 1).min(200);
                    let rows = ((clip.height() / checker_size).ceil() as i32 + 1).min(200);
                    for row in 0..rows {
                        for col in 0..cols {
                            let rect = egui::Rect::from_min_size(
                                egui::pos2(
                                    atlas_rect.min.x + col as f32 * checker_size,
                                    atlas_rect.min.y + row as f32 * checker_size,
                                ),
                                egui::vec2(checker_size, checker_size),
                            )
                            .intersect(atlas_rect);
                            painter.rect_filled(
                                rect,
                                0.0,
                                if (row + col) % 2 == 0 { light } else { dark },
                            );
                        }
                    }
                }

                if let Some(tex) = &state.atlas_texture {
                    painter.image(
                        tex.id(),
                        atlas_rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }

                let mouse_pos = ui.input(|i| i.pointer.hover_pos());
                state.hovered_sprite = None;

                for (i, sprite) in state.sprites.iter().enumerate() {
                    let s_tl = transform(sprite.x, sprite.y);
                    let s_br = transform(sprite.x + sprite.w, sprite.y + sprite.h);
                    let sprite_rect = egui::Rect::from_min_max(s_tl, s_br);

                    let is_hovered = mouse_pos.map(|p| sprite_rect.contains(p)).unwrap_or(false);
                    if is_hovered {
                        state.hovered_sprite = Some(i);
                    }
                    let is_selected = state.selected_sprite == Some(i);

                    if state.show_grid || is_hovered || is_selected {
                        let color = if is_selected {
                            egui::Color32::from_rgba_unmultiplied(0, 200, 255, 180)
                        } else if is_hovered {
                            egui::Color32::from_rgba_unmultiplied(255, 200, 0, 140)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(100, 255, 100, 40)
                        };
                        let sw = if is_selected || is_hovered { 2.0 } else { 1.0 };
                        painter.rect_stroke(sprite_rect, 0.0, egui::Stroke::new(sw, color), egui::StrokeKind::Outside);
                        if is_hovered || is_selected {
                            painter.rect_filled(
                                sprite_rect, 0.0,
                                egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 30),
                            );
                        }
                    }

                    if state.show_names && state.zoom > 0.3 {
                        let font_size = (10.0 * state.zoom).clamp(6.0, 14.0);
                        let short = sprite.name.rsplit('/').next().unwrap_or(&sprite.name);
                        painter.text(
                            s_tl + egui::vec2(2.0, 2.0),
                            egui::Align2::LEFT_TOP,
                            short,
                            egui::FontId::proportional(font_size),
                            egui::Color32::WHITE,
                        );
                    }

                    if state.show_mesh {
                        draw_sprite_mesh(&painter, sprite, &transform);
                    }
                }

                if response.clicked() {
                    state.selected_sprite = state.hovered_sprite;
                }

                painter.rect_stroke(atlas_rect, 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
            });
        }
    }
}

enum DeferredDialogAction {
    SaveThenDo(MenuAction),
    Do(MenuAction),
}

#[derive(Clone)]
enum MenuAction {
    NewProject,
    OpenProject,
    SaveProject,
    SaveProjectAs,
    AddSprites,
    OpenAtlas,
    OpenRecent(PathBuf),
    ExportAs,
    GoHome,
    ShowAbout,
    ShowLicense,
    /// Undo / redo wrap PackerState::history. Defined as menu actions
    /// (rather than inline keyboard handlers) so the Edit menu can disable
    /// them when there's nothing to undo / redo.
    Undo,
    Redo,
    /// Manual pack request — flips PackerState::manual_pack so the auto-pack
    /// dispatcher fires the pack regardless of the auto_pack toggle.
    PackNow,
}

impl eframe::App for MJAtlasApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Load fonts once
        if !self.fonts_loaded {
            load_fonts(ctx);
            self.fonts_loaded = true;
        }

        // Apply theme
        if self.dark_mode {
            apply_dark_theme(ctx);
        } else {
            apply_light_theme(ctx);
        }

        // Drain the hfrog probe / cloud list receivers. The probe always
        // resolves within ~1.5s; cloud list takes a single round trip after
        // that. While they're in flight we keep `request_repaint` so the
        // spinner doesn't freeze.
        self.poll_probe_result();
        self.poll_cloud_list_result();
        if self.probe_rx.is_some() || self.cloud_list_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        // Process keyboard shortcuts BEFORE any UI is laid out so a shortcut
        // doesn't race with widget event handling that would otherwise
        // consume the same key (e.g. Cmd+S inside a focused text field).
        self.handle_shortcuts(ctx);

        // Snapshot the project for undo on every frame where it actually
        // changed. Cheap PartialEq compare; a new entry is only cloned when
        // the user truly edited something.
        if let AppMode::Packer(state) = &mut self.mode {
            state.history.maybe_record(&state.project);
        }

        // Global menu bar on all screens
        self.ui_menubar(ctx);

        // Main content
        match &self.mode {
            AppMode::Welcome => self.ui_welcome(ctx),
            AppMode::Packer(_) => self.ui_packer(ctx),
            AppMode::Viewer(_) => self.ui_viewer(ctx),
        }

        // Dialogs overlay
        self.ui_dialogs(ctx);

        // Toast notifications overlay
        self.ui_toasts(ctx);
    }
}

// (PackerState::new_empty() replaces Default)

// ─── Launch functions ───

// Window minimum: roughly 1:2:1 aspect across sprite-list / canvas / settings.
// At this floor each side panel sits at its min_width (200 + 280) leaving
// ~480 px for the central canvas — enough for a useful preview. Any smaller
// and the canvas would degenerate to a sliver, so we refuse to shrink past it.
const MIN_WINDOW_INNER_SIZE: [f32; 2] = [960.0, 600.0];

pub fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("mj_atlas v{}", VERSION))
            .with_inner_size([1100.0, 750.0])
            .with_min_inner_size(MIN_WINDOW_INNER_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "mj_atlas",
        options,
        Box::new(|_cc| Ok(Box::new(MJAtlasApp::default()))),
    )
    .map_err(|e| AppError::Custom(format!("GUI error: {}", e)))?;
    Ok(())
}

pub fn run_preview(file: &Path) -> Result<()> {
    let viewer = load_atlas_for_viewer(file)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("mj_atlas v{} — {}", VERSION, file.display()))
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size(MIN_WINDOW_INNER_SIZE),
        ..Default::default()
    };
    eframe::run_native(
        "mj_atlas",
        options,
        Box::new(|_cc| Ok(Box::new(MJAtlasApp::with_viewer(viewer)))),
    )
    .map_err(|e| AppError::Custom(format!("GUI error: {}", e)))?;
    Ok(())
}

// ─── Helpers ───

// ─── Reusable UI components ───

/// Draw sprite list. Returns `true` if the sprite list was modified.
fn draw_sprite_list(
    ui: &mut egui::Ui,
    project: &mut Project,
    search_text: &mut String,
    dirty: &mut bool,
    _thumbnails: &mut HashMap<String, (egui::TextureHandle, u32, u32)>,
) -> bool {
    let mut changed = false;
    // Header row: heading only. The search box and buttons go on their own
    // row below so a narrow panel can't cause widgets to overlap (pre-v0.3.6
    // the header packed heading + search + 2 buttons in a single row, which
    // visibly collided once the user dragged the divider down to ~250 px).
    ui.heading(format!("Sprites ({})", project.sprites.len()));
    ui.horizontal(|ui| {
        // Buttons take fixed (intrinsic) width; the search box claims whatever
        // is left. Reserve ~140 px for the buttons + a small margin.
        let buttons_reserved = 140.0;
        let search_w = (ui.available_width() - buttons_reserved).max(80.0);
        ui.add_sized(
            [search_w, 22.0],
            egui::TextEdit::singleline(search_text).hint_text("Search..."),
        );
        if ui.button("+ Add").clicked() {
            if let Some(files) = rfd::FileDialog::new()
                .add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tga", "webp"])
                .pick_files()
            {
                for path in &files {
                    let ps = path.display().to_string();
                    if !project.sprites.contains(&ps) {
                        project.sprites.push(ps);
                    }
                }
                *dirty = true;
                changed = true;
            }
        }
        if ui.button("Clear").clicked() {
            project.sprites.clear();
            *dirty = true;
            changed = true;
        }
    });
    ui.separator();

    let mut remove_idx: Option<usize> = None;
    let search = search_text.to_lowercase();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (i, sprite_path) in project.sprites.iter().enumerate() {
            let filename = Path::new(sprite_path)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| sprite_path.clone());

            if !search.is_empty() && !filename.to_lowercase().contains(&search) {
                continue;
            }

            ui.horizontal(|ui| {
                let exists = Path::new(sprite_path).exists();
                let dot_color = if exists {
                    egui::Color32::from_rgb(80, 200, 80)
                } else {
                    egui::Color32::from_rgb(220, 60, 60)
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot_color);

                // Filenames render in JetBrains Mono so paths line up
                // visually — much easier to scan a long sprite list when
                // every character occupies the same column width.
                ui.label(egui::RichText::new(&filename).monospace());

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("x").clicked() {
                        remove_idx = Some(i);
                    }
                });
            });
        }

        ui.add_space(12.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Drop files here...").color(egui::Color32::DARK_GRAY).italics());
        });
    });

    if let Some(idx) = remove_idx {
        project.sprites.remove(idx);
        *dirty = true;
        changed = true;
    }

    changed
}

fn draw_inline_preview(ui: &mut egui::Ui, preview: &mut InlinePreview) {
    // Ensure texture loaded
    if preview.texture.is_none() {
        let size = [preview.atlas_w as usize, preview.atlas_h as usize];
        let pixels: Vec<egui::Color32> = preview.image.pixels()
            .map(|p| egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]))
            .collect();
        preview.texture = Some(ui.ctx().load_texture(
            "inline_atlas",
            egui::ColorImage { size, pixels },
            egui::TextureOptions::NEAREST,
        ));
    }

    // Toolbar
    //
    // Stability note: the hover info label MUST always be allocated, even
    // when there's no hover, otherwise the toolbar's row height fluctuates
    // by 1-2 px per frame depending on hover state. That fluctuation cascaded
    // into `ui.available_size()` for the canvas below, shifting the rendered
    // atlas every time the mouse crossed a sprite boundary — a visible jitter
    // (v0.3.3 bug). We pin the row height AND always render the slot, only
    // varying the label text.
    ui.horizontal(|ui| {
        ui.set_min_height(24.0);
        // Numeric / dimension labels in monospace so digits line up across
        // panels and zoom levels (e.g. "256x256" vs "1024x512" don't shift
        // surrounding widgets when the user re-packs at a different size).
        ui.label(egui::RichText::new(format!("{}x{}", preview.atlas_w, preview.atlas_h)).monospace());
        ui.separator();
        ui.label("Zoom:");
        ui.add(egui::Slider::new(&mut preview.zoom, 0.01..=10.0).logarithmic(true).show_value(false));
        ui.label(egui::RichText::new(format!("{:>3.0}%", preview.zoom * 100.0)).monospace());
        ui.checkbox(&mut preview.show_grid, "Grid");
        ui.checkbox(&mut preview.show_names, "Names");
        // Only meaningful when polygon mesh is present; the checkbox is harmless
        // otherwise (toggling it just doesn't draw anything).
        ui.checkbox(&mut preview.show_mesh, "Mesh");
        if ui.button("Fit").clicked() {
            preview.needs_fit = true;
        }
        ui.separator();
        let hover_text = preview
            .hovered
            .and_then(|i| preview.sprites.get(i))
            .map(|s| format!("{} ({}x{})", s.name, s.w as u32, s.h as u32))
            .unwrap_or_default();
        ui.label(egui::RichText::new(hover_text).monospace().small());
    });

    // Canvas
    let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
    let center = response.rect.center();

    // Auto-fit: compute zoom to fit atlas in canvas with margin
    if preview.needs_fit {
        let canvas_w = response.rect.width();
        let canvas_h = response.rect.height();
        if canvas_w > 10.0 && canvas_h > 10.0 && preview.atlas_w > 0 && preview.atlas_h > 0 {
            let margin = 20.0;
            let zoom_x = (canvas_w - margin * 2.0) / preview.atlas_w as f32;
            let zoom_y = (canvas_h - margin * 2.0) / preview.atlas_h as f32;
            preview.zoom = zoom_x.min(zoom_y).max(0.01);
            // Center: offset so atlas center maps to canvas center
            preview.pan_offset = egui::vec2(
                -(preview.atlas_w as f32) / 2.0,
                -(preview.atlas_h as f32) / 2.0,
            );
        }
        preview.needs_fit = false;
    }

    // Zoom
    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
    if scroll != 0.0 && response.hovered() {
        preview.zoom = (preview.zoom * (scroll * 0.005).exp()).clamp(0.05, 20.0);
    }

    // Pan
    if response.dragged_by(egui::PointerButton::Middle)
        || response.dragged_by(egui::PointerButton::Secondary)
        || (response.dragged_by(egui::PointerButton::Primary) && ui.input(|i| i.modifiers.shift))
    {
        preview.pan_offset += response.drag_delta() / preview.zoom;
    }

    let xf = |x: f32, y: f32| -> egui::Pos2 {
        egui::pos2(
            center.x + (x + preview.pan_offset.x) * preview.zoom,
            center.y + (y + preview.pan_offset.y) * preview.zoom,
        )
    };

    let tl = xf(0.0, 0.0);
    let br = xf(preview.atlas_w as f32, preview.atlas_h as f32);
    let atlas_rect = egui::Rect::from_min_max(tl, br);

    // Checkerboard
    let cs = 8.0 * preview.zoom;
    let clip = response.rect.intersect(atlas_rect);
    if clip.is_positive() {
        let cols = ((clip.width() / cs).ceil() as i32 + 1).min(200);
        let rows = ((clip.height() / cs).ceil() as i32 + 1).min(200);
        for row in 0..rows {
            for col in 0..cols {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(atlas_rect.min.x + col as f32 * cs, atlas_rect.min.y + row as f32 * cs),
                    egui::vec2(cs, cs),
                ).intersect(atlas_rect);
                let c = if (row + col) % 2 == 0 { egui::Color32::from_gray(140) } else { egui::Color32::from_gray(100) };
                painter.rect_filled(rect, 0.0, c);
            }
        }
    }

    // Atlas image
    if let Some(tex) = &preview.texture {
        painter.image(tex.id(), atlas_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    }

    // Sprite rects
    let mouse_pos = ui.input(|i| i.pointer.hover_pos());
    preview.hovered = None;

    for (i, sprite) in preview.sprites.iter().enumerate() {
        let s_tl = xf(sprite.x, sprite.y);
        let s_br = xf(sprite.x + sprite.w, sprite.y + sprite.h);
        let sr = egui::Rect::from_min_max(s_tl, s_br);

        let hovered = mouse_pos.map(|p| sr.contains(p)).unwrap_or(false);
        if hovered { preview.hovered = Some(i); }

        if preview.show_grid || hovered {
            let color = if hovered {
                egui::Color32::from_rgba_unmultiplied(255, 200, 0, 160)
            } else {
                egui::Color32::from_rgba_unmultiplied(100, 255, 100, 40)
            };
            let sw = if hovered { 2.0 } else { 1.0 };
            painter.rect_stroke(sr, 0.0, egui::Stroke::new(sw, color), egui::StrokeKind::Outside);
            if hovered {
                painter.rect_filled(sr, 0.0, egui::Color32::from_rgba_unmultiplied(255, 200, 0, 25));
            }
        }

        if preview.show_names && preview.zoom > 0.4 {
            let font_size = (10.0 * preview.zoom).clamp(6.0, 14.0);
            let short = sprite.name.rsplit('/').next().unwrap_or(&sprite.name);
            painter.text(s_tl + egui::vec2(2.0, 2.0), egui::Align2::LEFT_TOP, short, egui::FontId::proportional(font_size), egui::Color32::WHITE);
        }

        if preview.show_mesh {
            draw_sprite_mesh(&painter, sprite, &xf);
        }
    }

    painter.rect_stroke(atlas_rect, 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
}

/// Draw the polygon-mesh wireframe overlay for a sprite — semi-transparent
/// triangle edges in atlas-space, transformed via the canvas mapping function.
/// No-op when the sprite has no mesh data attached.
fn draw_sprite_mesh<F: Fn(f32, f32) -> egui::Pos2>(
    painter: &egui::Painter,
    sprite: &SpriteInfo,
    xf: &F,
) {
    let verts = match &sprite.mesh_vertices {
        Some(v) if v.len() >= 3 => v,
        _ => return,
    };
    let tris = match &sprite.mesh_triangles {
        Some(t) if !t.is_empty() => t,
        _ => return,
    };

    let edge_color = egui::Color32::from_rgba_unmultiplied(255, 80, 200, 200);
    let stroke = egui::Stroke::new(1.0, edge_color);

    for tri in tris {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        if a >= verts.len() || b >= verts.len() || c >= verts.len() {
            continue;
        }
        let pa = xf(verts[a][0], verts[a][1]);
        let pb = xf(verts[b][0], verts[b][1]);
        let pc = xf(verts[c][0], verts[c][1]);
        painter.line_segment([pa, pb], stroke);
        painter.line_segment([pb, pc], stroke);
        painter.line_segment([pc, pa], stroke);
    }
}

/// Find the common parent directory of a list of file paths.
fn find_common_parent(paths: &[String]) -> String {
    if paths.is_empty() {
        return ".".to_string();
    }
    if let Some(parent) = Path::new(&paths[0]).parent() {
        let parent_str = parent.display().to_string();
        if paths.iter().all(|p| p.starts_with(&parent_str)) {
            return parent_str;
        }
    }
    // Fallback: find longest common prefix
    let mut common = PathBuf::from(&paths[0]);
    for p in &paths[1..] {
        let path = PathBuf::from(p);
        let mut new_common = PathBuf::new();
        for (a, b) in common.components().zip(path.components()) {
            if a == b {
                new_common.push(a);
            } else {
                break;
            }
        }
        common = new_common;
    }
    if common.as_os_str().is_empty() {
        ".".to_string()
    } else {
        common.display().to_string()
    }
}

fn load_atlas_for_viewer(file: &Path) -> Result<ViewerState> {
    let content = std::fs::read_to_string(file)?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| AppError::Custom(format!("JSON parse: {}", e)))?;

    let dir = file.parent().unwrap_or(Path::new("."));
    let (image_path, sprites, animations) = if json.get("textures").is_some() {
        parse_tpsheet(&json, dir)?
    } else if json.get("frames").is_some() {
        parse_json_hash(&json, dir)?
    } else {
        return Err(AppError::Custom("Unsupported format".to_string()));
    };

    let atlas_image = image::open(&image_path)
        .map_err(|e| AppError::Custom(format!("Load '{}': {}", image_path.display(), e)))?
        .into_rgba8();
    let (atlas_w, atlas_h) = atlas_image.dimensions();
    let atlas_name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();

    Ok(ViewerState {
        atlas_texture: None,
        atlas_image,
        sprites,
        animations,
        atlas_name,
        atlas_w,
        atlas_h,
        zoom: 1.0,
        pan_offset: egui::Vec2::ZERO,
        hovered_sprite: None,
        selected_sprite: None,
        show_grid: true,
        show_names: false,
        show_mesh: false,
        search_text: String::new(),
        source_path: file.display().to_string(),
    })
}

fn parse_tpsheet(
    json: &serde_json::Value,
    dir: &Path,
) -> Result<(PathBuf, Vec<SpriteInfo>, HashMap<String, Vec<String>>)> {
    let tex = json["textures"].as_array().and_then(|a| a.first())
        .ok_or_else(|| AppError::Custom("No textures".to_string()))?;
    let image_path = dir.join(tex["image"].as_str().ok_or_else(|| AppError::Custom("Missing image".to_string()))?);
    let sprites: Vec<SpriteInfo> = tex["sprites"].as_array()
        .ok_or_else(|| AppError::Custom("Missing sprites".to_string()))?
        .iter()
        .filter_map(|s| {
            Some(SpriteInfo {
                name: s["filename"].as_str()?.to_string(),
                x: s["region"]["x"].as_f64()? as f32,
                y: s["region"]["y"].as_f64()? as f32,
                w: s["region"]["w"].as_f64()? as f32,
                h: s["region"]["h"].as_f64()? as f32,
                rotated: s["rotated"].as_bool().unwrap_or(false),
                source_w: s["region"]["w"].as_f64()? as f32 + s["margin"]["w"].as_f64().unwrap_or(0.0) as f32,
                source_h: s["region"]["h"].as_f64()? as f32 + s["margin"]["h"].as_f64().unwrap_or(0.0) as f32,
                mesh_vertices: parse_mesh_uv(s.get("verticesUV")),
                mesh_triangles: parse_mesh_triangles(s.get("triangles")),
            })
        })
        .collect();
    Ok((image_path, sprites, HashMap::new()))
}

fn parse_json_hash(
    json: &serde_json::Value,
    dir: &Path,
) -> Result<(PathBuf, Vec<SpriteInfo>, HashMap<String, Vec<String>>)> {
    let image_path = dir.join(json["meta"]["image"].as_str().ok_or_else(|| AppError::Custom("Missing meta.image".to_string()))?);
    let frames = json["frames"].as_object().ok_or_else(|| AppError::Custom("Missing frames".to_string()))?;
    let sprites: Vec<SpriteInfo> = frames.iter().filter_map(|(name, v)| {
        Some(SpriteInfo {
            name: name.clone(),
            x: v["frame"]["x"].as_f64()? as f32,
            y: v["frame"]["y"].as_f64()? as f32,
            w: v["frame"]["w"].as_f64()? as f32,
            h: v["frame"]["h"].as_f64()? as f32,
            rotated: v["rotated"].as_bool().unwrap_or(false),
            source_w: v["sourceSize"]["w"].as_f64()? as f32,
            source_h: v["sourceSize"]["h"].as_f64()? as f32,
            mesh_vertices: parse_mesh_uv(v.get("verticesUV")),
            mesh_triangles: parse_mesh_triangles(v.get("triangles")),
        })
    }).collect();
    let animations = json.get("animations")
        .and_then(|a| serde_json::from_value::<HashMap<String, Vec<String>>>(a.clone()).ok())
        .unwrap_or_default();
    Ok((image_path, sprites, animations))
}

/// Parse `verticesUV` (atlas-space mesh vertices) from a sprite metadata entry.
/// Extract the host portion of an endpoint URL for the connection badge.
/// Strips `https://` / `http://` prefix and any trailing path so the menubar
/// reads "● Online · hfrog.gamesci-lite.com" rather than the full URL.
fn host_of(endpoint: &str) -> String {
    let mut s = endpoint.trim();
    for p in ["https://", "http://"] {
        if let Some(stripped) = s.strip_prefix(p) {
            s = stripped;
            break;
        }
    }
    let s = s.split('/').next().unwrap_or(s);
    s.to_string()
}

fn parse_mesh_uv(value: Option<&serde_json::Value>) -> Option<Vec<[f32; 2]>> {
    let arr = value?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let pair = v.as_array()?;
        if pair.len() < 2 {
            return None;
        }
        out.push([pair[0].as_f64()? as f32, pair[1].as_f64()? as f32]);
    }
    Some(out)
}

/// Parse `triangles` (index triples into mesh vertices).
fn parse_mesh_triangles(value: Option<&serde_json::Value>) -> Option<Vec<[usize; 3]>> {
    let arr = value?.as_array()?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let tri = v.as_array()?;
        if tri.len() < 3 {
            return None;
        }
        out.push([
            tri[0].as_u64()? as usize,
            tri[1].as_u64()? as usize,
            tri[2].as_u64()? as usize,
        ]);
    }
    Some(out)
}
