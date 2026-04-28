//! Block Composer — second OS window for designing a new BlockMeta
//! through iterative texgen prompts.
//!
//! ## What it is
//!
//! A standalone "creator" tool. The main editor stays the *modeler*
//! (paint cells, stack heights, export glTF). This window is the
//! *block author's* sandbox: pick a shape (cube / sphere), describe
//! the surface in natural language, generate a texture through
//! `texgen` (cpu/fal lane via Rustyme), see it on a single block,
//! iterate, and ultimately produce a `BlockMeta` record that either
//! gets stashed as a local draft (visible in the main window's Block
//! Library) or pushed up to the hfrog artifact server so the whole
//! team can `Sync` it.
//!
//! ## Why a separate OS window
//!
//! The compose loop is *long-form*: the user sits in front of the
//! prompt+regenerate cycle for minutes at a time. Stuffing it into
//! the main editor's right SidePanel would either
//!
//! * crowd out the existing Block Library + palette UI, or
//! * shrink the 3D preview viewport every time the composer is
//!   open.
//!
//! Spawning a second OS window fixes both: the user can park the
//! composer on a second monitor while painting on the main canvas.
//! `float_window.rs` already proves the pattern.
//!
//! ## Layout (per the brief)
//!
//! ```
//! ┌────────────────────────────────────────────────────────────┐
//! │ Block Composer                                              │
//! ├──────────────────────────┬─────────────────────────────────┤
//! │ ╭ Shape ╮                 │ Prompt:                          │
//! │ │ Cube  │  ← floating     │ ┌─────────────────────────────┐ │
//! │ │ Sphere│                  │ │ patchy moss-tipped grass... │ │
//! │ ╰───────╯                  │ └─────────────────────────────┘ │
//! │                            │ Provider: [cpu ▼]  Seed: [1]   │
//! │   ┌──── 3D ────┐           │ Style:    [auto▼]  Size:[128]  │
//! │   │  preview   │           │ [Generate]                       │
//! │   │  block     │           │ ─── History ───                 │
//! │   │  with png  │           │ [thumb] prompt 1 (cpu, seed 1)  │
//! │   └────────────┘           │ [thumb] prompt 2 (fal, seed 2)  │
//! │                            │ ─── Save / Publish ───          │
//! │                            │ id: [grass]  name: [...]        │
//! │                            │ [Save Local Draft] [Publish]    │
//! └──────────────────────────┴─────────────────────────────────┘
//! ```
//!
//! Three async tasks live behind the right panel:
//! * **Generate** — calls `texgen::TextureProvider::generate()` on a
//!   background thread (RustymeProvider or MockProvider). Returns
//!   PNG bytes, pushed onto the history.
//! * **Save** — writes a `BlockMeta` (with
//!   [`maquette::block_meta::BlockMetaSource::LocalDraft`]) plus the
//!   PNG to `~/.cache/maquette/blocks/local-drafts/<id>.{json,png}`.
//! * **Publish** — multipart `PUT /api/artifactory/add_form_file`
//!   against hfrog with the same payload. (`HfrogPublisher` is in
//!   `maquette::block_meta::hfrog::publisher`.)
//!
//! Headless invariant: the *data layer* (`BlockMeta`, providers,
//! cache, publisher) lives in the lib. This module is GUI-only; it
//! only orchestrates Bevy resources / messages / windows.

use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::{RenderTarget, Viewport};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::image::Image;
use bevy::math::UVec2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::{block_on, futures_lite::future, AsyncComputeTaskPool, Task};
use bevy::window::{
    PrimaryWindow, RequestRedraw, WindowClosed, WindowRef, WindowResolution,
};
use bevy_egui::{egui, EguiContext, EguiContexts, EguiMultipassSchedule};
use bevy_panorbit_camera::PanOrbitCamera;

use crate::camera::egui_rect;

/// Schedule label that runs the composer window's egui pass. Per
/// `bevy_egui` 0.39's multi-pass guidance: the second window needs
/// its own schedule so we don't nest UI systems inside the main
/// window's `EguiPrimaryContextPass`.
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComposerContextPass;

use maquette::block_meta::{
    drafts::{self, LocalDraftProvider},
    hfrog::{HfrogConfig, HfrogPublisher, PublishOutcome},
    {self, BlockMeta, BlockMetaSource, RgbaColor},
};
use maquette::grid::ShapeKind;
use maquette::texgen::{
    rustyme::{RustymeConfig, RustymeProfile, RustymeProvider},
    MockProvider, TextureProvider, TextureRequest,
};

use crate::block_library::BlockLibraryState;
use crate::notify::Toasts;

// ---------------------------------------------------------------------
// Resource + form types
// ---------------------------------------------------------------------

/// Logical-pixel rectangle the composer's 3-D camera should occupy
/// inside the second window. Mirrors [`crate::camera::PreviewViewportRect`]
/// for the main editor — written by `composer_ui_system` after egui
/// has claimed its panels, read by `sync_composer_viewport` to
/// scissor the camera. `None` = first frame / second window not
/// open.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct ComposerViewportRect {
    pub rect: Option<egui_rect::Rect>,
}

/// Top-level composer state. One instance per app — the second window
/// either renders this state or doesn't exist. Closing the window
/// only flips `visible = false` so the user can reopen it without
/// losing the in-flight conversation.
#[derive(Resource, Default)]
pub struct BlockComposerState {
    pub visible: bool,
    pub shape: ShapeKind,
    pub form: ComposerForm,
    pub draft: ComposerDraft,
    pub history: Vec<ComposerAttempt>,
    /// Index into `history` for the attempt currently applied to the
    /// preview block. `None` means "no attempt yet" (preview block
    /// shows its solid `default_color`).
    pub selected_attempt: Option<usize>,
    pub generating: bool,
    pub saving: bool,
    pub publishing: bool,
    /// Last error string from any of the three async tasks. Cleared
    /// on the next successful run.
    pub error: Option<String>,
    /// Monotonic id counter for `ComposerAttempt::id`.
    next_attempt_id: u64,
}

impl BlockComposerState {
    pub fn selected_attempt(&self) -> Option<&ComposerAttempt> {
        self.selected_attempt.and_then(|i| self.history.get(i))
    }

    pub fn busy(&self) -> bool {
        self.generating || self.saving || self.publishing
    }

    fn next_id(&mut self) -> u64 {
        self.next_attempt_id += 1;
        self.next_attempt_id
    }
}

/// Live form state for the next `Generate` click.
#[derive(Clone, Debug)]
pub struct ComposerForm {
    pub prompt: String,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub provider: ComposerProvider,
    pub style_mode: ComposerStyleMode,
}

impl Default for ComposerForm {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            seed: 1,
            width: 128,
            height: 128,
            provider: ComposerProvider::RustymeCpu,
            style_mode: ComposerStyleMode::Auto,
        }
    }
}

/// Texgen lane the user picks for the next attempt. Maps to the
/// `RustymeProfile` enum (`Cpu`/`Fal`) plus a non-network `Mock`
/// path so a build without `MAQUETTE_RUSTYME_REDIS_URL` can still
/// exercise the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerProvider {
    RustymeCpu,
    RustymeFal,
    Mock,
}

impl ComposerProvider {
    pub fn label(self) -> &'static str {
        match self {
            Self::RustymeCpu => "rustyme · cpu",
            Self::RustymeFal => "rustyme · fal",
            Self::Mock => "mock (offline)",
        }
    }
}

/// `cpu` lane only — what `style_mode` to send. `Unset` omits the
/// kwarg so the worker uses its own default; otherwise we pass the
/// string straight through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposerStyleMode {
    Auto,
    Solid,
    Smart,
    Unset,
}

impl ComposerStyleMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto (default)",
            Self::Solid => "solid (deterministic)",
            Self::Smart => "smart (LLM)",
            Self::Unset => "(omit kwarg)",
        }
    }
    pub fn to_kwarg(self) -> Option<String> {
        match self {
            Self::Auto => Some("auto".into()),
            Self::Solid => Some("solid".into()),
            Self::Smart => Some("smart".into()),
            Self::Unset => None,
        }
    }
}

/// One historical generate result. Owns its PNG bytes — small
/// enough (≤ 128 KB at 128² PNG) that holding the entire conversation
/// in memory is fine.
#[derive(Clone, Debug)]
#[allow(dead_code)] // most fields are read by the UI panel below
pub struct ComposerAttempt {
    pub id: u64,
    pub prompt: String,
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub provider: ComposerProvider,
    pub style_mode: ComposerStyleMode,
    pub png_bytes: Vec<u8>,
    pub generated_at: i64,
    /// Pre-decoded RGBA buffer for the preview-block texture — a
    /// `bevy::Image` is built from it the first time the user
    /// selects this attempt; cached after that. `None` until the
    /// first selection.
    pub texture_handle: Option<Handle<Image>>,
}

/// Editable BlockMeta draft. Populated by the user before they hit
/// Save / Publish. Sensible defaults filled from the latest
/// successful attempt's prompt.
#[derive(Default, Clone, Debug)]
pub struct ComposerDraft {
    pub id: String,
    pub name: String,
    pub description: String,
    pub texture_hint: String,
    /// Comma-separated tags. Split + trim on Save.
    pub tags: String,
}

// ---------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------

#[derive(Message, Clone, Debug, Default)]
pub struct OpenBlockComposer;

#[derive(Message, Clone, Debug, Default)]
pub struct CloseBlockComposer;

#[derive(Message, Clone, Copy, Debug)]
pub struct ComposerSetShape(pub ShapeKind);

/// Run a generate with the form's current snapshot.
#[derive(Message, Clone, Debug, Default)]
pub struct ComposerGenerate;

#[derive(Message, Clone, Copy, Debug)]
pub struct ComposerSelectAttempt(pub usize);

#[derive(Message, Clone, Copy, Debug)]
pub struct ComposerDiscardAttempt(pub usize);

#[derive(Message, Clone, Debug, Default)]
pub struct ComposerSaveDraft;

#[derive(Message, Clone, Debug, Default)]
pub struct ComposerPublish;

// ---------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------

#[derive(Component)]
pub struct BlockComposerWindow;

#[derive(Component)]
pub struct BlockComposerCamera;

#[derive(Component)]
pub struct BlockComposerPreviewMesh;

#[derive(Component)]
pub struct BlockComposerLight;

#[derive(Component)]
struct PendingGenerate {
    task: Task<GenerateOutcome>,
    /// Form snapshot at the moment of dispatch — the history entry
    /// is built from this so the user can change the form mid-
    /// generation without us recording stale parameters.
    form: ComposerForm,
}

#[derive(Component)]
struct PendingSave {
    task: Task<Result<std::path::PathBuf, String>>,
    draft_id: String,
}

#[derive(Component)]
struct PendingPublish {
    task: Task<Result<PublishOutcome, String>>,
    draft_id: String,
}

struct GenerateOutcome {
    bytes: Result<Vec<u8>, String>,
    elapsed_ms: u128,
}

// ---------------------------------------------------------------------
// Plugin wiring
// ---------------------------------------------------------------------

pub struct BlockComposerPlugin;

impl Plugin for BlockComposerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BlockComposerState>()
            .init_resource::<ComposerViewportRect>()
            .add_message::<OpenBlockComposer>()
            .add_message::<CloseBlockComposer>()
            .add_message::<ComposerSetShape>()
            .add_message::<ComposerGenerate>()
            .add_message::<ComposerSelectAttempt>()
            .add_message::<ComposerDiscardAttempt>()
            .add_message::<ComposerSaveDraft>()
            .add_message::<ComposerPublish>()
            .add_systems(
                Update,
                (
                    handle_open_close,
                    handle_window_closed,
                    apply_visibility,
                    handle_set_shape,
                    handle_select_attempt,
                    handle_discard_attempt,
                    handle_generate_request,
                    handle_save_request,
                    handle_publish_request,
                    poll_generate_tasks,
                    poll_save_tasks,
                    poll_publish_tasks,
                    sync_composer_viewport,
                )
                    .chain(),
            )
            // UI on the second window has its own egui context;
            // we tag the composer's camera with
            // `EguiMultipassSchedule::new(ComposerContextPass)` so
            // bevy_egui dispatches this schedule once per frame for
            // that camera's egui context.
            .add_systems(ComposerContextPass, composer_ui_system);
    }
}

// ---------------------------------------------------------------------
// Open / close lifecycle
// ---------------------------------------------------------------------

fn handle_open_close(
    mut open: MessageReader<OpenBlockComposer>,
    mut close: MessageReader<CloseBlockComposer>,
    mut state: ResMut<BlockComposerState>,
) {
    let mut to_open = !open.is_empty();
    open.clear();
    if to_open {
        if state.visible {
            // Already open — `apply_visibility` is idempotent;
            // nothing to do.
            to_open = false;
        }
        state.visible = true;
        state.error = None;
    }
    if !close.is_empty() {
        close.clear();
        state.visible = false;
    }
    let _ = to_open;
}

/// React to `WindowClosed` events for the composer window. Mirrors
/// `float_window::handle_float_window_closed`'s logic (we can't
/// reliably query for the marker component on the closed entity, so
/// we filter by "non-primary, non-floatpreview").
fn handle_window_closed(
    mut events: MessageReader<WindowClosed>,
    primary: Query<Entity, With<PrimaryWindow>>,
    composer_windows: Query<Entity, With<BlockComposerWindow>>,
    float_windows: Query<Entity, With<crate::float_window::FloatPreviewWindow>>,
    mut state: ResMut<BlockComposerState>,
) {
    let primary_entity = primary.iter().next();
    let float_entities: std::collections::HashSet<Entity> = float_windows.iter().collect();
    for ev in events.read() {
        if Some(ev.window) == primary_entity {
            continue;
        }
        if float_entities.contains(&ev.window) {
            continue;
        }
        // It's our window (or a window we don't recognise; flagging
        // visible=false here is safe either way since
        // apply_visibility despawns based on the marker query).
        if !composer_windows.is_empty() {
            state.visible = false;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_visibility(
    state: Res<BlockComposerState>,
    mut commands: Commands,
    composer_windows: Query<Entity, With<BlockComposerWindow>>,
    composer_cameras: Query<Entity, With<BlockComposerCamera>>,
    composer_meshes: Query<Entity, With<BlockComposerPreviewMesh>>,
    composer_lights: Query<Entity, With<BlockComposerLight>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !state.is_changed() {
        return;
    }

    if state.visible {
        if !composer_windows.is_empty() {
            return;
        }
        let window = commands
            .spawn((
                Window {
                    title: "Maquette · Block Composer".into(),
                    resolution: WindowResolution::new(1100, 720),
                    ..default()
                },
                BlockComposerWindow,
            ))
            .id();

        // Render target camera. Parked on a fixed orbit looking at
        // the world origin where we put the preview block.
        //
        // * `EguiMultipassSchedule` tells bevy_egui to drive the
        //   `ComposerContextPass` schedule for *this* camera's
        //   context. Without it, the camera renders fine but the
        //   second window's egui never gets a chance to run.
        // * `viewport: Some(1×1 placeholder)` matches the main
        //   preview's startup pattern (`camera::spawn_camera`):
        //   bevy keeps the camera around even when its viewport
        //   covers a single pixel, so on the very first frame —
        //   before `composer_ui_system` has reported the central
        //   rect — we render a single-pixel splash instead of
        //   filling the entire window underneath the side panel.
        //   The next frame `sync_composer_viewport` resizes us
        //   into the real central rect.
        commands.spawn((
            Name::new("Block Composer Camera"),
            Camera3d::default(),
            Camera {
                clear_color: bevy::camera::ClearColorConfig::Custom(
                    bevy::color::Color::srgb(0.10, 0.10, 0.12),
                ),
                viewport: Some(Viewport {
                    physical_position: UVec2::ZERO,
                    physical_size: UVec2::new(1, 1),
                    ..default()
                }),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
            Transform::default(),
            PanOrbitCamera {
                yaw: Some(-std::f32::consts::FRAC_PI_4),
                pitch: Some(std::f32::consts::FRAC_PI_8),
                radius: Some(2.6),
                target_yaw: -std::f32::consts::FRAC_PI_4,
                target_pitch: std::f32::consts::FRAC_PI_8,
                target_radius: 2.6,
                target_focus: Vec3::ZERO,
                focus: Vec3::ZERO,
                ..default()
            },
            EguiMultipassSchedule::new(ComposerContextPass),
            BlockComposerCamera,
        ));

        // A pair of directional + ambient lights so the block is
        // legible even before any texture lands. Lights are scoped
        // to the composer scene by `RenderLayers` not yet (we share
        // the world with the main scene); they are dim enough not
        // to disturb the main 3D preview's PBR look.
        commands.spawn((
            Name::new("Block Composer Light"),
            DirectionalLight {
                color: bevy::color::Color::WHITE,
                illuminance: 6_000.0,
                ..default()
            },
            Transform::from_xyz(2.0, 4.0, 3.0)
                .looking_at(Vec3::ZERO, Vec3::Y),
            BlockComposerLight,
        ));

        // The preview block itself. Material is plain white at
        // first; `apply_selected_attempt_to_material` swaps in the
        // generated texture later. We spawn a fresh standard
        // material so we never share one with the main scene.
        let mesh = meshes.add(make_mesh_for_shape(state.shape));
        let material = materials.add(StandardMaterial {
            base_color: bevy::color::Color::srgb(0.85, 0.85, 0.88),
            perceptual_roughness: 0.6,
            metallic: 0.0,
            ..default()
        });
        commands.spawn((
            Name::new("Block Composer Preview"),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
            BlockComposerPreviewMesh,
        ));
    } else {
        for e in &composer_meshes {
            commands.entity(e).despawn();
        }
        for e in &composer_lights {
            commands.entity(e).despawn();
        }
        for e in &composer_cameras {
            commands.entity(e).despawn();
        }
        for e in &composer_windows {
            commands.entity(e).despawn();
        }
    }
}

fn make_mesh_for_shape(shape: ShapeKind) -> Mesh {
    match shape {
        ShapeKind::Cube => Mesh::from(Cuboid::new(1.0, 1.0, 1.0)),
        ShapeKind::Sphere => Mesh::from(Sphere::new(0.55)),
    }
}

// ---------------------------------------------------------------------
// Shape switching
// ---------------------------------------------------------------------

fn handle_set_shape(
    mut events: MessageReader<ComposerSetShape>,
    mut state: ResMut<BlockComposerState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<&mut Mesh3d, With<BlockComposerPreviewMesh>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(target) = events.read().last().map(|e| e.0) else {
        return;
    };
    if state.shape == target {
        return;
    }
    state.shape = target;
    if let Some(mut mesh3d) = q.iter_mut().next() {
        let new_mesh = meshes.add(make_mesh_for_shape(target));
        *mesh3d = Mesh3d(new_mesh);
    }
    redraw.write(RequestRedraw);
}

// ---------------------------------------------------------------------
// Generate dispatch + polling
// ---------------------------------------------------------------------

fn handle_generate_request(
    mut events: MessageReader<ComposerGenerate>,
    mut state: ResMut<BlockComposerState>,
    mut commands: Commands,
    mut toasts: ResMut<Toasts>,
) {
    if events.is_empty() {
        return;
    }
    events.clear();
    if state.generating {
        toasts.info("Generate already in progress");
        return;
    }
    if state.form.prompt.trim().is_empty() {
        state.error = Some("prompt is empty".to_string());
        toasts.error("Prompt is empty");
        return;
    }

    state.generating = true;
    state.error = None;

    let form = state.form.clone();
    let form_for_record = form.clone();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let started = std::time::Instant::now();
        let bytes = run_generate_blocking(form);
        GenerateOutcome {
            bytes,
            elapsed_ms: started.elapsed().as_millis(),
        }
    });
    commands.spawn(PendingGenerate {
        task,
        form: form_for_record,
    });
}

fn run_generate_blocking(form: ComposerForm) -> Result<Vec<u8>, String> {
    let request = TextureRequest::new(
        form.prompt.clone(),
        form.seed,
        form.width,
        form.height,
        match form.provider {
            ComposerProvider::Mock => MockProvider::MODEL_ID.to_string(),
            ComposerProvider::RustymeCpu | ComposerProvider::RustymeFal => {
                std::env::var("MAQUETTE_RUSTYME_MODEL")
                    .unwrap_or_else(|_| "rustyme:texture.gen".to_string())
            }
        },
    );
    match form.provider {
        ComposerProvider::Mock => {
            let provider = MockProvider;
            provider
                .generate(&request)
                .map(|b| b.0)
                .map_err(|e| e.to_string())
        }
        ComposerProvider::RustymeCpu | ComposerProvider::RustymeFal => {
            let mut cfg = match RustymeConfig::from_env() {
                Some(c) => c,
                None => {
                    return Err(
                        "rustyme provider needs MAQUETTE_RUSTYME_REDIS_URL — \
                         set it (and MAQUETTE_RUSTYME_ADMIN_URL for revoke) \
                         and try again, or pick provider=mock for an offline run."
                            .into(),
                    );
                }
            };
            let profile = match form.provider {
                ComposerProvider::RustymeCpu => RustymeProfile::Cpu,
                ComposerProvider::RustymeFal => RustymeProfile::Fal,
                ComposerProvider::Mock => unreachable!(),
            };
            // Override the keys to match the chosen lane explicitly,
            // ignoring any `MAQUETTE_RUSTYME_QUEUE_KEY` the user may
            // have set globally.
            cfg.queue_key = profile.queue_key().to_string();
            cfg.result_key = profile.result_key().to_string();
            cfg.style_mode = form.style_mode.to_kwarg();
            let provider = RustymeProvider::new(cfg);
            provider
                .generate(&request)
                .map(|b| b.0)
                .map_err(|e| e.to_string())
        }
    }
}

fn poll_generate_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingGenerate)>,
    mut state: ResMut<BlockComposerState>,
    mut toasts: ResMut<Toasts>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    for (entity, mut pending) in &mut tasks {
        if let Some(outcome) = block_on(future::poll_once(&mut pending.task)) {
            commands.entity(entity).despawn();
            state.generating = false;
            match outcome.bytes {
                Ok(bytes) => {
                    let now = unix_seconds();
                    let attempt = ComposerAttempt {
                        id: state.next_id(),
                        prompt: pending.form.prompt.clone(),
                        seed: pending.form.seed,
                        width: pending.form.width,
                        height: pending.form.height,
                        provider: pending.form.provider,
                        style_mode: pending.form.style_mode,
                        png_bytes: bytes,
                        generated_at: now,
                        texture_handle: None,
                    };
                    let new_idx = state.history.len();
                    state.history.push(attempt);
                    // Auto-select latest so the preview updates.
                    state.selected_attempt = Some(new_idx);
                    if state.draft.texture_hint.is_empty() {
                        state.draft.texture_hint = state.form.prompt.clone();
                    }
                    toasts.info(format!(
                        "generated {}×{} in {:.2}s",
                        pending.form.width,
                        pending.form.height,
                        outcome.elapsed_ms as f32 / 1000.0
                    ));
                }
                Err(msg) => {
                    log::warn!("composer: generate failed: {msg}");
                    toasts.error(format!("Generate failed: {msg}"));
                    state.error = Some(msg);
                }
            }
            redraw.write(RequestRedraw);
        }
    }
}

// ---------------------------------------------------------------------
// History selection / discard
// ---------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn handle_select_attempt(
    mut events: MessageReader<ComposerSelectAttempt>,
    mut state: ResMut<BlockComposerState>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    q: Query<&MeshMaterial3d<StandardMaterial>, With<BlockComposerPreviewMesh>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(idx) = events.read().last().map(|e| e.0) else {
        return;
    };
    if idx >= state.history.len() {
        return;
    }
    state.selected_attempt = Some(idx);

    // Decode + cache an Image asset for the attempt if we haven't.
    let png_bytes = state.history[idx].png_bytes.clone();
    let handle = if let Some(h) = state.history[idx].texture_handle.clone() {
        h
    } else {
        match decode_png_to_image(&png_bytes) {
            Ok(image) => {
                let h = images.add(image);
                state.history[idx].texture_handle = Some(h.clone());
                h
            }
            Err(e) => {
                log::warn!("composer: png decode failed: {e}");
                return;
            }
        }
    };

    // Apply onto the preview block's material.
    if let Some(mat_handle) = q.iter().next() {
        if let Some(material) = materials.get_mut(&mat_handle.0) {
            material.base_color_texture = Some(handle);
            // White base_color so the texture shows through; the
            // unlit-ish look comes from the directional + ambient.
            material.base_color = bevy::color::Color::WHITE;
        }
    }
    redraw.write(RequestRedraw);
}

fn decode_png_to_image(bytes: &[u8]) -> Result<Image, String> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("png header: {e}"))?;
    // png 0.18 returns `Option<usize>` here — `None` for streams
    // whose declared dimensions overflow usize (we never see this in
    // practice because images come from texgen-cpu / fal worker).
    let buf_size = reader
        .output_buffer_size()
        .ok_or_else(|| "png: output_buffer_size overflowed".to_string())?;
    let mut buf = vec![0; buf_size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| format!("png decode: {e}"))?;

    // Normalise to RGBA8 — most of our worker outputs already are,
    // but a stray RGB or palette image shouldn't hard-fail.
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity(info.buffer_size() * 4 / 3);
            for c in buf.chunks(3) {
                out.extend_from_slice(c);
                out.push(255);
            }
            out
        }
        other => return Err(format!("unsupported color_type={other:?}")),
    };
    Ok(Image::new(
        Extent3d {
            width: info.width,
            height: info.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

fn handle_discard_attempt(
    mut events: MessageReader<ComposerDiscardAttempt>,
    mut state: ResMut<BlockComposerState>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let Some(idx) = events.read().last().map(|e| e.0) else {
        return;
    };
    if idx >= state.history.len() {
        return;
    }
    state.history.remove(idx);
    // Adjust selected_attempt: shift left by one if removing
    // before/at it; clear if it was the very one removed.
    state.selected_attempt = match state.selected_attempt {
        Some(sel) if sel == idx => None,
        Some(sel) if sel > idx => Some(sel - 1),
        other => other,
    };
    redraw.write(RequestRedraw);
}

// ---------------------------------------------------------------------
// Save (local draft)
// ---------------------------------------------------------------------

fn handle_save_request(
    mut events: MessageReader<ComposerSaveDraft>,
    mut state: ResMut<BlockComposerState>,
    mut commands: Commands,
    mut toasts: ResMut<Toasts>,
) {
    if events.is_empty() {
        return;
    }
    events.clear();
    if state.saving {
        toasts.info("Save already in progress");
        return;
    }
    if state.publishing {
        toasts.info("Wait for publish to finish first");
        return;
    }

    let Some(attempt) = state.selected_attempt() else {
        toasts.error("Select an attempt before saving");
        return;
    };
    let attempt = attempt.clone();
    let draft = state.draft.clone();
    let validated = match validate_draft(&draft) {
        Ok(v) => v,
        Err(e) => {
            toasts.error(e.clone());
            state.error = Some(e);
            return;
        }
    };

    state.saving = true;
    state.error = None;
    let shape_at_dispatch = state.shape;
    let draft_id = validated.id.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        write_local_draft(&validated, &draft, &attempt, shape_at_dispatch)
    });
    commands.spawn(PendingSave {
        task,
        draft_id,
    });
}

#[derive(Clone, Debug)]
struct ValidatedDraft {
    id: String,
}

fn validate_draft(draft: &ComposerDraft) -> Result<ValidatedDraft, String> {
    let id = draft.id.trim();
    if id.is_empty() {
        return Err("Block id is empty".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(format!(
            "Block id `{id}` contains illegal chars (must be [a-z0-9_])"
        ));
    }
    Ok(ValidatedDraft { id: id.to_string() })
}

fn write_local_draft(
    validated: &ValidatedDraft,
    draft: &ComposerDraft,
    attempt: &ComposerAttempt,
    shape: ShapeKind,
) -> Result<std::path::PathBuf, String> {
    let cache_dir = block_meta::default_cache_dir().ok_or_else(|| {
        "no cache dir — set XDG_CACHE_HOME or HOME and retry".to_string()
    })?;
    let meta = BlockMeta {
        id: validated.id.clone(),
        name: trim_or_id(&draft.name, &validated.id),
        description: draft.description.trim().to_string(),
        shape_hint: shape,
        default_color: dominant_color_or_grey(&attempt.png_bytes),
        texture_hint: trim_or_prompt(&draft.texture_hint, &attempt.prompt),
        tags: draft
            .tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        source: BlockMetaSource::LocalDraft {
            created_at: unix_seconds(),
        },
        preview_s3_key: None,
    };
    drafts::write_draft(&cache_dir, &meta, &attempt.png_bytes).map_err(|e| e.to_string())
}

fn trim_or_id(name: &str, id: &str) -> String {
    let t = name.trim();
    if t.is_empty() {
        id.to_string()
    } else {
        t.to_string()
    }
}
fn trim_or_prompt(hint: &str, prompt: &str) -> String {
    let t = hint.trim();
    if t.is_empty() {
        prompt.trim().to_string()
    } else {
        t.to_string()
    }
}

/// Naive 16-pixel sampler for the preview-block fallback colour. Not
/// a real "average colour" — we just pick the first non-transparent
/// pixel near the centre and call that representative. Good enough
/// for the BlockMeta `default_color` field, which is itself a
/// fallback when the texture isn't loaded yet.
fn dominant_color_or_grey(png_bytes: &[u8]) -> RgbaColor {
    let Ok(img) = decode_png_to_image(png_bytes) else {
        return RgbaColor::rgb(0.6, 0.6, 0.6);
    };
    let w = img.texture_descriptor.size.width as usize;
    let h = img.texture_descriptor.size.height as usize;
    if w == 0 || h == 0 {
        return RgbaColor::rgb(0.6, 0.6, 0.6);
    }
    let center = ((h / 2) * w + (w / 2)) * 4;
    let data = &img.data.as_ref().unwrap();
    if data.len() < center + 4 {
        return RgbaColor::rgb(0.6, 0.6, 0.6);
    }
    RgbaColor {
        r: data[center] as f32 / 255.0,
        g: data[center + 1] as f32 / 255.0,
        b: data[center + 2] as f32 / 255.0,
        a: 1.0,
    }
}

fn poll_save_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingSave)>,
    mut state: ResMut<BlockComposerState>,
    mut toasts: ResMut<Toasts>,
    mut library: ResMut<BlockLibraryState>,
) {
    for (entity, mut pending) in &mut tasks {
        if let Some(result) = block_on(future::poll_once(&mut pending.task)) {
            commands.entity(entity).despawn();
            state.saving = false;
            match result {
                Ok(path) => {
                    log::info!("composer: saved draft {}", path.display());
                    toasts.info(format!(
                        "Saved local draft `{}`. Visible in the main window's Block Library.",
                        pending.draft_id
                    ));
                    // Hot-refresh the main window's library so the
                    // user sees the new draft without restarting.
                    if let Ok(merged) =
                        LocalDraftProvider::new().merge_into_library(&library.blocks)
                    {
                        library.blocks = merged;
                    }
                }
                Err(msg) => {
                    log::warn!("composer: save failed: {msg}");
                    toasts.error(format!("Save failed: {msg}"));
                    state.error = Some(msg);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Publish (hfrog)
// ---------------------------------------------------------------------

fn handle_publish_request(
    mut events: MessageReader<ComposerPublish>,
    mut state: ResMut<BlockComposerState>,
    mut commands: Commands,
    mut toasts: ResMut<Toasts>,
) {
    if events.is_empty() {
        return;
    }
    events.clear();
    if state.publishing {
        toasts.info("Publish already in progress");
        return;
    }
    if state.saving {
        toasts.info("Wait for save to finish first");
        return;
    }

    let Some(attempt) = state.selected_attempt() else {
        toasts.error("Select an attempt before publishing");
        return;
    };
    let attempt = attempt.clone();
    let draft = state.draft.clone();
    let shape = state.shape;
    let validated = match validate_draft(&draft) {
        Ok(v) => v,
        Err(e) => {
            toasts.error(e.clone());
            state.error = Some(e);
            return;
        }
    };

    state.publishing = true;
    state.error = None;
    let draft_id = validated.id.clone();

    let task = AsyncComputeTaskPool::get().spawn(async move {
        publish_draft_blocking(&validated, &draft, &attempt, shape)
    });
    commands.spawn(PendingPublish {
        task,
        draft_id,
    });
}

fn publish_draft_blocking(
    validated: &ValidatedDraft,
    draft: &ComposerDraft,
    attempt: &ComposerAttempt,
    shape: ShapeKind,
) -> Result<PublishOutcome, String> {
    let meta = BlockMeta {
        id: validated.id.clone(),
        name: trim_or_id(&draft.name, &validated.id),
        description: draft.description.trim().to_string(),
        shape_hint: shape,
        default_color: dominant_color_or_grey(&attempt.png_bytes),
        texture_hint: trim_or_prompt(&draft.texture_hint, &attempt.prompt),
        tags: draft
            .tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        // The publisher overwrites this with `BlockMetaSource::Hfrog`
        // populated from the server response.
        source: BlockMetaSource::LocalDraft {
            created_at: unix_seconds(),
        },
        preview_s3_key: None,
    };
    let cfg = HfrogConfig::from_env();
    let publisher = HfrogPublisher::new(cfg);
    publisher
        .publish_block(&meta, &attempt.png_bytes)
        .map_err(|e| e.to_string())
}

fn poll_publish_tasks(
    mut commands: Commands,
    mut tasks: Query<(Entity, &mut PendingPublish)>,
    mut state: ResMut<BlockComposerState>,
    mut toasts: ResMut<Toasts>,
) {
    for (entity, mut pending) in &mut tasks {
        if let Some(result) = block_on(future::poll_once(&mut pending.task)) {
            commands.entity(entity).despawn();
            state.publishing = false;
            match result {
                Ok(outcome) => {
                    log::info!(
                        "composer: published `{}` to hfrog (pid={})",
                        pending.draft_id,
                        outcome.pid
                    );
                    toasts.info(format!(
                        "Published `{}` to hfrog (pid={}). Run `Sync hfrog` in the main library to fan out.",
                        pending.draft_id, outcome.pid
                    ));
                    // Best-effort: drop the local draft now that the
                    // server has it. If the deletion fails we leave it
                    // alone — the user can re-publish without harm.
                    if let Some(cache_dir) = block_meta::default_cache_dir() {
                        let _ = drafts::remove_draft(&cache_dir, &pending.draft_id);
                    }
                }
                Err(msg) => {
                    log::warn!("composer: publish failed: {msg}");
                    toasts.error(format!("Publish failed: {msg}"));
                    state.error = Some(msg);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Helper used by the egui panel that wants a snapshot of the
/// resource without holding a live borrow.
#[allow(dead_code)]
pub fn snapshot(state: &BlockComposerState) -> Arc<BlockComposerState> {
    // The state is fairly small (the heaviest field is `history`'s
    // PNG bytes — bounded by what the user has generated this
    // session). `Arc::new` clones the whole thing.
    Arc::new(BlockComposerState {
        visible: state.visible,
        shape: state.shape,
        form: state.form.clone(),
        draft: state.draft.clone(),
        history: state.history.clone(),
        selected_attempt: state.selected_attempt,
        generating: state.generating,
        saving: state.saving,
        publishing: state.publishing,
        error: state.error.clone(),
        next_attempt_id: state.next_attempt_id,
    })
}

// ---------------------------------------------------------------------
// UI — runs on the second window's EguiContext.
// ---------------------------------------------------------------------

/// Bundle the messages the composer UI emits so we don't fight the
/// 16-param ceiling.
#[derive(bevy::ecs::system::SystemParam)]
pub struct ComposerMessages<'w> {
    set_shape: MessageWriter<'w, ComposerSetShape>,
    generate: MessageWriter<'w, ComposerGenerate>,
    select: MessageWriter<'w, ComposerSelectAttempt>,
    discard: MessageWriter<'w, ComposerDiscardAttempt>,
    save: MessageWriter<'w, ComposerSaveDraft>,
    publish: MessageWriter<'w, ComposerPublish>,
    redraw: MessageWriter<'w, RequestRedraw>,
}

#[allow(clippy::too_many_arguments)]
fn composer_ui_system(
    mut contexts: Query<&mut EguiContext, With<BlockComposerWindow>>,
    mut state: ResMut<BlockComposerState>,
    mut viewport_rect: ResMut<ComposerViewportRect>,
    mut msgs: ComposerMessages,
) {
    let Ok(mut ctx_handle) = contexts.single_mut() else {
        return;
    };
    let ctx = ctx_handle.get_mut();

    // ---- Floating "Shape" picker over the 3D preview (left side) ----
    egui::Area::new(egui::Id::new("composer_shape_picker"))
        .anchor(egui::Align2::LEFT_TOP, egui::vec2(16.0, 16.0))
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::group(ui.style())
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(8))
                .fill(egui::Color32::from_black_alpha(180))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Shape").strong().small());
                    ui.horizontal(|ui| {
                        let cube_selected = matches!(state.shape, ShapeKind::Cube);
                        if ui
                            .selectable_label(cube_selected, "Cube")
                            .on_hover_text("Solid 1×1×1 voxel block")
                            .clicked()
                            && !cube_selected
                        {
                            msgs.set_shape.write(ComposerSetShape(ShapeKind::Cube));
                        }
                        let sphere_selected = matches!(state.shape, ShapeKind::Sphere);
                        if ui
                            .selectable_label(sphere_selected, "Sphere")
                            .on_hover_text("Sphere placeholder — same prompt, different geometry")
                            .clicked()
                            && !sphere_selected
                        {
                            msgs.set_shape.write(ComposerSetShape(ShapeKind::Sphere));
                        }
                    });
                });
        });

    // ---- Right SidePanel: prompt form + history + draft + actions ----
    egui::SidePanel::right("composer_right_panel")
        .default_width(380.0)
        .min_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Block Composer");
                ui.label(
                    egui::RichText::new(
                        "Iterate on a single block's surface texture; \
                         pick the best attempt, fill the draft form, \
                         then save locally or publish to hfrog.",
                    )
                    .small(),
                );
                ui.separator();

                // === Prompt + provider form ===
                ui.label(egui::RichText::new("Prompt").strong());
                let prompt_resp = ui.add(
                    egui::TextEdit::multiline(&mut state.form.prompt)
                        .hint_text("e.g. patchy moss-tipped grass, hand-painted, top-down lighting")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                if prompt_resp.changed() {
                    msgs.redraw.write(RequestRedraw);
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Provider:");
                    egui::ComboBox::from_id_salt("composer_provider")
                        .selected_text(state.form.provider.label())
                        .show_ui(ui, |ui| {
                            for choice in [
                                ComposerProvider::RustymeCpu,
                                ComposerProvider::RustymeFal,
                                ComposerProvider::Mock,
                            ] {
                                if ui
                                    .selectable_label(
                                        state.form.provider == choice,
                                        choice.label(),
                                    )
                                    .clicked()
                                {
                                    state.form.provider = choice;
                                }
                            }
                        });
                });
                if matches!(state.form.provider, ComposerProvider::RustymeCpu) {
                    ui.horizontal(|ui| {
                        ui.label("Style mode:");
                        egui::ComboBox::from_id_salt("composer_style_mode")
                            .selected_text(state.form.style_mode.label())
                            .show_ui(ui, |ui| {
                                for choice in [
                                    ComposerStyleMode::Auto,
                                    ComposerStyleMode::Solid,
                                    ComposerStyleMode::Smart,
                                    ComposerStyleMode::Unset,
                                ] {
                                    if ui
                                        .selectable_label(
                                            state.form.style_mode == choice,
                                            choice.label(),
                                        )
                                        .clicked()
                                    {
                                        state.form.style_mode = choice;
                                    }
                                }
                            });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Seed:");
                    let mut seed_str = state.form.seed.to_string();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut seed_str).desired_width(80.0),
                        )
                        .changed()
                    {
                        if let Ok(s) = seed_str.parse() {
                            state.form.seed = s;
                        }
                    }
                    if ui.small_button("↻").on_hover_text("Bump seed by one").clicked() {
                        state.form.seed = state.form.seed.wrapping_add(1);
                    }
                    ui.add_space(8.0);
                    ui.label("Size:");
                    let mut w_str = state.form.width.to_string();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut w_str).desired_width(50.0),
                        )
                        .changed()
                    {
                        if let Ok(v) = w_str.parse::<u32>() {
                            state.form.width = v.clamp(16, 1024);
                            state.form.height = state.form.width;
                        }
                    }
                    ui.label("²");
                });

                ui.add_space(6.0);
                let busy = state.busy();
                let prompt_empty = state.form.prompt.trim().is_empty();
                let gen_label = if state.generating {
                    "Generating…"
                } else {
                    "Generate"
                };
                if ui
                    .add_enabled(
                        !busy && !prompt_empty,
                        egui::Button::new(gen_label).min_size(egui::vec2(120.0, 28.0)),
                    )
                    .on_disabled_hover_text(if prompt_empty {
                        "Type a prompt first."
                    } else {
                        "Another async task is in flight; wait for it to finish."
                    })
                    .clicked()
                {
                    msgs.generate.write(ComposerGenerate);
                    msgs.redraw.write(RequestRedraw);
                }
                if let Some(err) = &state.error {
                    ui.label(
                        egui::RichText::new(format!("Last error: {err}"))
                            .small()
                            .color(egui::Color32::from_rgb(220, 90, 90)),
                    );
                }

                ui.separator();

                // === History timeline ===
                ui.label(egui::RichText::new("History").strong());
                if state.history.is_empty() {
                    ui.label(
                        egui::RichText::new("(no attempts yet)")
                            .italics()
                            .small(),
                    );
                } else {
                    let selected = state.selected_attempt;
                    let mut to_select: Option<usize> = None;
                    let mut to_discard: Option<usize> = None;
                    let mut to_replay: Option<ComposerForm> = None;
                    for (i, attempt) in state.history.iter().enumerate().rev() {
                        let active = selected == Some(i);
                        let frame = egui::Frame::group(ui.style())
                            .stroke(if active {
                                egui::Stroke::new(
                                    2.0,
                                    egui::Color32::from_rgb(120, 180, 255),
                                )
                            } else {
                                egui::Stroke::new(1.0, egui::Color32::from_gray(80))
                            });
                        frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("#{}", attempt.id))
                                        .small()
                                        .strong(),
                                );
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · seed {} · {}×{}",
                                        attempt.provider.label(),
                                        attempt.seed,
                                        attempt.width,
                                        attempt.height
                                    ))
                                    .small()
                                    .color(egui::Color32::from_gray(160)),
                                );
                            });
                            ui.label(
                                egui::RichText::new(truncate_str(&attempt.prompt, 110))
                                    .italics(),
                            );
                            ui.horizontal(|ui| {
                                if ui
                                    .add_enabled(
                                        !active,
                                        egui::Button::new(if active {
                                            "✔ Selected"
                                        } else {
                                            "Select"
                                        }),
                                    )
                                    .clicked()
                                {
                                    to_select = Some(i);
                                }
                                if ui
                                    .add_enabled(!busy, egui::Button::new("Replay"))
                                    .on_hover_text("Re-run with the same prompt + provider, bumping the seed")
                                    .clicked()
                                {
                                    let mut form = state.form.clone();
                                    form.prompt = attempt.prompt.clone();
                                    form.provider = attempt.provider;
                                    form.style_mode = attempt.style_mode;
                                    form.width = attempt.width;
                                    form.height = attempt.height;
                                    form.seed = attempt.seed.wrapping_add(1);
                                    to_replay = Some(form);
                                }
                                if ui.button("Discard").clicked() {
                                    to_discard = Some(i);
                                }
                            });
                        });
                    }
                    if let Some(i) = to_select {
                        msgs.select.write(ComposerSelectAttempt(i));
                    }
                    if let Some(i) = to_discard {
                        msgs.discard.write(ComposerDiscardAttempt(i));
                    }
                    if let Some(form) = to_replay {
                        state.form = form;
                        msgs.generate.write(ComposerGenerate);
                        msgs.redraw.write(RequestRedraw);
                    }
                }

                ui.separator();

                // === Draft form ===
                ui.label(egui::RichText::new("Save / Publish").strong());
                ui.label(
                    egui::RichText::new(
                        "Pick a Selected attempt above first. Block id is \
                         lower-case + underscores ([a-z0-9_]).",
                    )
                    .small()
                    .color(egui::Color32::from_gray(160)),
                );

                egui::Grid::new("composer_draft_grid")
                    .num_columns(2)
                    .spacing([6.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("id");
                        ui.text_edit_singleline(&mut state.draft.id);
                        ui.end_row();
                        ui.label("name");
                        ui.text_edit_singleline(&mut state.draft.name);
                        ui.end_row();
                        ui.label("description");
                        ui.add(
                            egui::TextEdit::multiline(&mut state.draft.description)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                        ui.label("texture_hint");
                        ui.add(
                            egui::TextEdit::multiline(&mut state.draft.texture_hint)
                                .hint_text("(defaults to selected attempt's prompt)")
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                        ui.end_row();
                        ui.label("tags");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.draft.tags)
                                .hint_text("comma,separated"),
                        );
                        ui.end_row();
                    });

                ui.add_space(6.0);
                let has_selection = state.selected_attempt.is_some();
                let id_valid = !state.draft.id.trim().is_empty();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !busy && has_selection && id_valid,
                            egui::Button::new(if state.saving {
                                "Saving…"
                            } else {
                                "Save Local Draft"
                            }),
                        )
                        .on_disabled_hover_text(disabled_reason(
                            busy,
                            has_selection,
                            id_valid,
                        ))
                        .clicked()
                    {
                        msgs.save.write(ComposerSaveDraft);
                    }
                    if ui
                        .add_enabled(
                            !busy && has_selection && id_valid,
                            egui::Button::new(if state.publishing {
                                "Publishing…"
                            } else {
                                "Publish to Hfrog"
                            }),
                        )
                        .on_disabled_hover_text(disabled_reason(
                            busy,
                            has_selection,
                            id_valid,
                        ))
                        .clicked()
                    {
                        msgs.publish.write(ComposerPublish);
                    }
                });
                ui.label(
                    egui::RichText::new(
                        "Local draft → ~/.cache/maquette/blocks/local-drafts/<id>.{json,png}\n\
                         Publish → MAQUETTE_HFROG_BASE_URL (default \
                         https://starlink.youxi123.com/hfrog)",
                    )
                    .small()
                    .color(egui::Color32::from_gray(140)),
                );
            });
        });

    // The CentralPanel has nothing to draw — the 3D preview camera
    // owns the back of the window. We still need to claim the
    // central area (a) so egui doesn't fight it for layout, and
    // (b) so we can capture its `available_rect_before_wrap()` —
    // *that* rectangle becomes the camera viewport, scissoring
    // the 3D render to exactly the leftover central region.
    let central_rect = egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| ui.available_rect_before_wrap())
        .inner;

    // Stash the rect so `sync_composer_viewport` can read it next
    // frame. We deliberately update *every* frame (no
    // `is_changed()` guard) because `available_rect_before_wrap`
    // is a fresh pointer-derived value each frame; comparing for
    // change would be cheap but the resource write itself is too,
    // so spend the cycles for code clarity.
    *viewport_rect = ComposerViewportRect {
        rect: Some(egui_rect::Rect {
            min_x: central_rect.min.x,
            min_y: central_rect.min.y,
            max_x: central_rect.max.x,
            max_y: central_rect.max.y,
        }),
    };
}

/// Mirror of [`crate::camera::sync_main_viewport`] for the composer
/// camera. Without this the second window's `Camera3d` renders into
/// the entire window — including the area covered by the right
/// `SidePanel` — which is exactly the "right side looks chaotic"
/// symptom: 3-D content drawn behind egui leaks through wherever the
/// SidePanel has any transparency, and the orbit center sits at the
/// physical-window middle (well to the right of where the user sees
/// "the preview area").
fn sync_composer_viewport(
    rect: Res<ComposerViewportRect>,
    state: Res<BlockComposerState>,
    windows: Query<&Window, With<BlockComposerWindow>>,
    mut cams: Query<&mut Camera, With<BlockComposerCamera>>,
) {
    if !state.visible {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut cam) = cams.single_mut() else {
        return;
    };

    let scale = window.scale_factor();
    let phys = window.physical_size();

    // Same fallback shape as the main viewport sync: if egui hasn't
    // reported a rect yet, occupy the whole window so the user sees
    // *something* rather than the 1×1 splash spawn placeholder.
    let (pos, size) = match rect.rect {
        Some(r) if r.max_x > r.min_x && r.max_y > r.min_y => {
            let px = (r.min_x * scale).round().max(0.0) as u32;
            let py = (r.min_y * scale).round().max(0.0) as u32;
            let pw = ((r.max_x - r.min_x) * scale).round().max(1.0) as u32;
            let ph = ((r.max_y - r.min_y) * scale).round().max(1.0) as u32;
            let pw = pw.min(phys.x.saturating_sub(px)).max(1);
            let ph = ph.min(phys.y.saturating_sub(py)).max(1);
            (UVec2::new(px, py), UVec2::new(pw, ph))
        }
        _ => (UVec2::ZERO, phys.max(UVec2::new(1, 1))),
    };

    if let Some(vp) = cam.viewport.as_mut() {
        if vp.physical_position != pos || vp.physical_size != size {
            vp.physical_position = pos;
            vp.physical_size = size;
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_chars {
        s.to_string()
    } else {
        let mut t: String = chars[..max_chars.saturating_sub(1)].iter().collect();
        t.push('…');
        t
    }
}

fn disabled_reason(busy: bool, has_selection: bool, id_valid: bool) -> &'static str {
    if busy {
        "Another async task is in flight; wait for it to finish."
    } else if !has_selection {
        "Pick a Selected attempt above first."
    } else if !id_valid {
        "Type a block id (e.g. `mossy_stone`)."
    } else {
        ""
    }
}

/// Suppress the unused-import warning if we end up shaving down the
/// imports later — bevy_egui's `EguiContexts` and `EguiContext` are
/// both required (the latter as a type bound on the query) but the
/// linter doesn't always reason that way through cfg gates.
#[allow(dead_code)]
fn _unused_imports_anchor(_c: EguiContexts) {}
