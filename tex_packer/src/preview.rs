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

#[derive(Serialize, Deserialize, Clone, Debug)]
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

#[derive(Serialize, Deserialize, Clone, Debug)]
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
    /// Flag: auto-pack needed on next frame (set by drop/add)
    needs_auto_pack: bool,
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
    search_text: String,
    source_path: String,
}

// ─── Main App ───

struct TexPackerApp {
    mode: AppMode,
    toasts: Vec<Toast>,
    open_dialog: Option<Dialog>,
    recent_files: Vec<PathBuf>,
    dark_mode: bool,
    fonts_loaded: bool,
}

const FORMAT_NAMES: &[(&str, &str)] = &[
    ("json", "TexturePacker JSON Hash"),
    ("json-array", "TexturePacker JSON Array"),
    ("godot-tpsheet", "Godot .tpsheet"),
    ("godot-tres", "Godot native .tres"),
];

const VERSION: &str = env!("CARGO_PKG_VERSION");

impl Default for TexPackerApp {
    fn default() -> Self {
        Self {
            mode: AppMode::Packer(PackerState::new_empty()),
            toasts: Vec::new(),
            open_dialog: None,
            recent_files: Vec::new(),
            dark_mode: false,
            fonts_loaded: false,
        }
    }
}

impl PackerState {
    fn new_empty() -> Self {
        Self {
            project: Project::default(),
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
        }
    }

    fn from_project(project: Project, path: PathBuf) -> Self {
        let needs_pack = !project.sprites.is_empty();
        Self {
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

/// Load Inter + JetBrains Mono fonts into egui.
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

    // Set Inter as primary proportional font (before defaults for fallback)
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());

    // Set JetBrains Mono as primary monospace
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains".to_owned());

    ctx.set_fonts(fonts);
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

impl TexPackerApp {
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

        egui::TopBottomPanel::top("main_menu").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        action = Some(MenuAction::NewProject);
                        ui.close_menu();
                    }
                    if ui.button("Open Project...").clicked() {
                        action = Some(MenuAction::OpenProject);
                        ui.close_menu();
                    }

                    ui.separator();

                    ui.add_enabled_ui(in_packer, |ui| {
                        if ui.button("Save Project").clicked() {
                            action = Some(MenuAction::SaveProject);
                            ui.close_menu();
                        }
                        if ui.button("Save Project As...").clicked() {
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
                        if ui.button("Export As...").clicked() {
                            action = Some(MenuAction::ExportAs);
                            ui.close_menu();
                        }
                    });
                });

                ui.menu_button("View", |ui| {
                    if ui.checkbox(&mut self.dark_mode, "Dark Mode").changed() {
                        ui.close_menu();
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About tex_packer").clicked() {
                        action = Some(MenuAction::ShowAbout);
                        ui.close_menu();
                    }
                    if ui.button("License").clicked() {
                        action = Some(MenuAction::ShowLicense);
                        ui.close_menu();
                    }
                });

                // Right-aligned project info
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let status = match &self.mode {
                        AppMode::Welcome => "Home".to_string(),
                        AppMode::Packer(s) => format!("{} | {} sprites", s.title(), s.project.sprites.len()),
                        AppMode::Viewer(_) => "Preview".to_string(),
                    };
                    ui.label(
                        egui::RichText::new(format!("tex_packer v{} | {}", VERSION, status))
                            .small()
                            .color(egui::Color32::GRAY),
                    );
                });
            });
        });

        if let Some(action) = action {
            self.handle_menu_action(action);
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
                    .add_filter("tex_packer project", &["tpproj"])
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
        }
    }

    fn is_packer_dirty(&self) -> bool {
        matches!(&self.mode, AppMode::Packer(s) if s.dirty)
    }

    fn save_project(&mut self, save_as: bool) {
        if let AppMode::Packer(state) = &mut self.mode {
            let path = if save_as || state.project_path.is_none() {
                rfd::FileDialog::new()
                    .add_filter("tex_packer project", &["tpproj"])
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
                    egui::Window::new("About tex_packer")
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.add_space(8.0);
                                ui.heading(
                                    egui::RichText::new("tex_packer")
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
                                ui.label(egui::RichText::new("tex_packer").strong().size(16.0));
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
            if let AppMode::Viewer(state) = &self.mode {
                // Re-read the source file and re-export
                let source = PathBuf::from(&state.source_path);
                if let Ok(content) = std::fs::read_to_string(&source) {
                    // For now, just copy/convert — a simple re-export
                    if let Err(e) = std::fs::write(&path, &content) {
                        self.toast(format!("Export failed: {}", e), ToastKind::Error);
                    } else {
                        self.toast(
                            format!("Exported to {}", path.display()),
                            ToastKind::Success,
                        );
                    }
                }
            }
        }
    }

    // ─── Welcome screen ──

    fn ui_welcome(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading(egui::RichText::new("tex_packer").size(40.0).strong());
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

                if !self.recent_files.is_empty() {
                    ui.add_space(20.0);
                    ui.label(egui::RichText::new("Recent").size(13.0).color(egui::Color32::GRAY));
                    ui.add_space(4.0);
                    let mut open_path: Option<PathBuf> = None;
                    for path in &self.recent_files {
                        let label = path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                        if ui.add(egui::Label::new(
                            egui::RichText::new(&label).size(13.0).color(egui::Color32::from_rgb(100, 180, 255)),
                        ).sense(egui::Sense::click())).clicked() {
                            open_path = Some(path.clone());
                        }
                    }
                    if let Some(path) = open_path {
                        self.handle_menu_action(MenuAction::OpenRecent(path));
                    }
                }
            });
        });
    }

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
                incremental: false,
                quantize: s.quantize,
                quantize_quality: s.quantize_quality,
                polygon: s.polygon,
                tolerance: s.tolerance,
            };

            let (tx, rx) = std::sync::mpsc::channel();
            let ctx_clone = ctx.clone();
            std::thread::spawn(move || {
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
                    Err(e) => BackgroundPackResult::Error(format!("Pack: {}", e)),
                };
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

        // Trigger auto-pack (must be outside the borrow above)
        let should_auto_pack = matches!(&self.mode, AppMode::Packer(s) if s.needs_auto_pack && s.pack_rx.is_none() && !s.project.sprites.is_empty());
        if should_auto_pack {
            if let AppMode::Packer(state) = &mut self.mode {
                state.needs_auto_pack = false;
            }
            self.trigger_pack(ctx);
        }

        let is_packing_now = matches!(&self.mode, AppMode::Packer(s) if s.pack_rx.is_some());
        let list_changed = std::cell::Cell::new(false);

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
            egui::SidePanel::right("settings_panel")
                .default_width(300.0)
                .min_width(250.0)
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
                        ui.horizontal(|ui| {
                            ui.label("Max size:");
                            egui::ComboBox::from_id_salt("max_size")
                                .selected_text(format!("{}", s.max_size))
                                .show_ui(ui, |ui| {
                                    for sz in [256, 512, 1024, 2048, 4096, 8192] {
                                        if ui.selectable_value(&mut s.max_size, sz, format!("{}", sz)).changed() {
                                            state.dirty = true;
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Spacing:");
                            ui.add(egui::DragValue::new(&mut s.spacing).range(0..=32));
                            ui.label("Pad:");
                            ui.add(egui::DragValue::new(&mut s.padding).range(0..=32));
                            ui.label("Extr:");
                            ui.add(egui::DragValue::new(&mut s.extrude).range(0..=8));
                        });
                        ui.checkbox(&mut s.trim, "Trim transparent");
                        ui.checkbox(&mut s.rotate, "Allow rotation");
                        ui.checkbox(&mut s.pot, "Power-of-2");
                        ui.checkbox(&mut s.polygon, "Polygon mesh");
                        if s.polygon {
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.label("Tolerance:");
                                ui.add(egui::DragValue::new(&mut s.tolerance).range(0.5..=10.0).speed(0.1));
                            });
                        }
                        ui.checkbox(&mut s.quantize, "PNG quantize");
                        if s.quantize {
                            ui.horizontal(|ui| {
                                ui.add_space(20.0);
                                ui.label("Quality:");
                                ui.add(egui::DragValue::new(&mut s.quantize_quality).range(1..=100));
                            });
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
                                ui.label(egui::RichText::new(
                                    format!("Atlas: {}x{}, {} sprites", p.atlas_w, p.atlas_h, p.sprites.len())
                                ).small().color(egui::Color32::GRAY));
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
                            state.needs_auto_pack = true;
                        }

                        if is_packing {
                            ui.spinner();
                        }
                    });
                });

            // ── Central area: sprite list + preview ──
            let has_preview = state.preview.is_some();
            let split = state.split_dir;

            // When we have a preview in horizontal mode, use a left SidePanel for sprite list
            if has_preview && split == SplitDir::Horizontal {
                egui::SidePanel::left("sprite_list_panel")
                    .default_width(280.0)
                    .min_width(200.0)
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
                egui::TopBottomPanel::top("sprite_list_panel_v")
                    .default_height(200.0)
                    .min_height(100.0)
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
            egui::SidePanel::left("sprite_list")
                .default_width(260.0)
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
                            let label = format!(
                                "{} ({}x{}{})",
                                sprite.name,
                                sprite.w as u32,
                                sprite.h as u32,
                                if sprite.rotated { " R" } else { "" }
                            );
                            if ui.selectable_label(is_selected, &label).clicked() {
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
            egui::TopBottomPanel::bottom("viewer_controls").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Zoom:");
                    ui.add(egui::Slider::new(&mut state.zoom, 0.1..=10.0).logarithmic(true));
                    ui.checkbox(&mut state.show_grid, "Grid");
                    ui.checkbox(&mut state.show_names, "Names");
                    if ui.button("Fit").clicked() {
                        state.zoom = 1.0;
                        state.pan_offset = egui::Vec2::ZERO;
                        state.selected_sprite = None;
                    }

                    if let Some(idx) = state.hovered_sprite {
                        let s = &state.sprites[idx];
                        ui.separator();
                        ui.label(format!(
                            "{} @ ({},{}) {}x{} src:{}x{}",
                            s.name, s.x as u32, s.y as u32,
                            s.w as u32, s.h as u32,
                            s.source_w as u32, s.source_h as u32
                        ));
                    }
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
    #[allow(dead_code)]
    GoHome,
    ShowAbout,
    ShowLicense,
}

impl eframe::App for TexPackerApp {
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

pub fn run_gui() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("tex_packer v{}", VERSION))
            .with_inner_size([1100.0, 750.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tex_packer",
        options,
        Box::new(|_cc| Ok(Box::new(TexPackerApp::default()))),
    )
    .map_err(|e| AppError::Custom(format!("GUI error: {}", e)))?;
    Ok(())
}

pub fn run_preview(file: &Path) -> Result<()> {
    let viewer = load_atlas_for_viewer(file)?;
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("tex_packer v{} — {}", VERSION, file.display()))
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tex_packer",
        options,
        Box::new(|_cc| Ok(Box::new(TexPackerApp::with_viewer(viewer)))),
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
    ui.horizontal(|ui| {
        ui.heading(format!("Sprites ({})", project.sprites.len()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("Clear").clicked() {
                project.sprites.clear();
                *dirty = true;
                changed = true;
            }
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
            ui.add_sized([100.0, 18.0], egui::TextEdit::singleline(search_text).hint_text("Search..."));
        });
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

                ui.label(&filename);

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
    ui.horizontal(|ui| {
        ui.label(format!("{}x{}", preview.atlas_w, preview.atlas_h));
        ui.separator();
        ui.label("Zoom:");
        ui.add(egui::Slider::new(&mut preview.zoom, 0.01..=10.0).logarithmic(true).show_value(false));
        ui.label(format!("{:.0}%", preview.zoom * 100.0));
        ui.checkbox(&mut preview.show_grid, "Grid");
        ui.checkbox(&mut preview.show_names, "Names");
        if ui.button("Fit").clicked() {
            preview.needs_fit = true;
        }
        if let Some(idx) = preview.hovered {
            ui.separator();
            let s = &preview.sprites[idx];
            ui.label(egui::RichText::new(format!(
                "{} ({}x{})", s.name, s.w as u32, s.h as u32
            )).small());
        }
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
    }

    painter.rect_stroke(atlas_rect, 0.0, egui::Stroke::new(1.0, egui::Color32::WHITE), egui::StrokeKind::Outside);
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
        })
    }).collect();
    let animations = json.get("animations")
        .and_then(|a| serde_json::from_value::<HashMap<String, Vec<String>>>(a.clone()).ok())
        .unwrap_or_default();
    Ok((image_path, sprites, animations))
}
