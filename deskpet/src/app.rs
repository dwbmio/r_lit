//! Bevy 桌面宠物应用体 — 由 `lib::launch_with_inbox` 启动。

use std::time::Duration;

use bevy::prelude::*;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::window::{
    CompositeAlphaMode, CursorOptions, Monitor, PrimaryWindow, WindowLevel, WindowPosition,
    WindowResolution,
};
use bevy::asset::AssetPlugin;
use bevy::ecs::system::NonSendMarker;
use bevy::gltf::GltfAssetLabel;
use bevy::winit::{UpdateMode, WinitSettings, WINIT_WINDOWS};
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};
use bevy_tray_icon::plugin::menu_event::MenuMessage;
use bevy_tray_icon::plugin::TrayIconPlugin;
use bevy_tray_icon::resource::{Menu, MenuItem, TrayIcon};
use bevy_tray_icon::{
    MouseButton as TrayMouseButton, MouseButtonState as TrayMouseButtonState, TrayIconEvent,
};
use rand::Rng;

use crate::notify::NotifyState;
use crate::os_notify::{OsNotifyPlugin, make_notifier};
use crate::rpc::bevy_bridge::RpcTaskInbox;
use bevy::log::LogPlugin;

// ============================================================================
//  SIZE / LAYOUT KNOBS — tune these to taste while debugging the pet.
// ============================================================================
//
// The window is partitioned: a pet area on the left holds the 3D mascot, and
// an egui HUD panel sits on the right. The whole window is one rigid body —
// dragging the mascot moves the HUD with it.

/// Visible mascot area width (logical px). Shrink this to shrink the slime.
const PET_W: f32 = 150.0;
/// Window height (logical px).
const WIN_H: f32 = 200.0;
/// Expanded HUD panel width (logical px). The HUD area is transparent and
/// click-through while collapsed, so it costs no screen space until opened.
const HUD_W: f32 = 160.0;
/// HUD widget/text size multiplier (buttons, sliders, gear, labels).
const UI_SCALE: f32 = 1.2;
// ============================================================================

/// Total logical window dimensions (pixels).
const WIN_W: f32 = PET_W + HUD_W;
/// Mascot grab radius (logical px), centered on the left pet area where the
/// camera frames the mascot (offset left, clear of the right-side HUD).
const HIT_R: f32 = PET_W * 0.45;
/// Horizontal walking speed, physical px/sec.
const WALK_SPEED: f32 = 90.0;
/// Jump impulse / gravity for the hop on click, in *world* units.
const JUMP_V: f32 = 3.2;
const GRAVITY: f32 = 14.0;
/// World-space body radius of the slime.
const BODY_W: f32 = 1.0;
/// Yaw the mascot turns to when facing left/right while walking (radians).
const FACE_YAW: f32 = 0.45;

/// 主模型（idle 网格 + idle 动画）；行走动画见 [`MASCOT_WALK_GLB`].
const MASCOT_GLB: &str = "mascot.glb";
/// 行走循环动画（仅取 Animation 0，与主模型同骨骼）.
const MASCOT_WALK_GLB: &str = "mascot_walk.glb";

/// True if the default mascot GLB exists under the resolved assets directory.
fn mascot_glb_exists(assets_dir: &std::path::Path) -> bool {
    assets_dir.join(MASCOT_GLB).is_file()
        && assets_dir.join(MASCOT_WALK_GLB).is_file()
}

// ---- Adaptive (lazy) frame rate -------------------------------------------
// We never render at a fixed high rate when nothing is happening. The loop is
// `Reactive`, and a system rewrites the wait interval every tick based on
// activity, so an idle, untouched mascot costs almost nothing.

/// ~60 fps while moving / dragging / hovered / animating.
const ACTIVE_WAIT: Duration = Duration::from_micros(16_667);
/// ~30 fps when the pointer is approaching the window (snappy to grab).
const NEAR_WAIT: Duration = Duration::from_millis(33);
/// ~8 fps idle heartbeat: just enough to poll the global cursor for hover and
/// advance the blink timer. This is the floor cost of a perched mascot.
const IDLE_WAIT: Duration = Duration::from_millis(120);
/// Screen-space radius (logical px) around the pet center within which the
/// pointer counts as "approaching".
const NEAR_R: f32 = PET_W * 1.4;

/// Collapsed HUD: a gear button sits at the top-right corner of the pet area.
/// Click-region dimensions scale with `UI_SCALE` so they track the gear size.
const GEAR_W: f32 = 30.0 * UI_SCALE;
const GEAR_H: f32 = 30.0 * UI_SCALE;

/// Logical center of the mascot body within the window (top-left origin). The
/// camera renders the full window but is offset so the mascot sits in the left
/// pet area, clear of the right-side HUD.
fn pet_center() -> Vec2 {
    Vec2::new(PET_W * 0.5, WIN_H * 0.5)
}

/// Reminder bubble: a top-anchored speech bubble drawn over the upper part of
/// the window when a reminder is active. Its zone is added to the interactive
/// hit-test so a click anywhere on it dismisses the reminder.
const BUBBLE_H: f32 = WIN_H * 0.5;

const HELP: &str = "\
deskpet — frameless transparent always-on-top 3D mascot (Bevy)

USAGE:
    deskpet                       Launch the mascot (default).
    deskpet help                  Print this help.
    deskpet send [OPTIONS] [BODY] Push a reminder. Alias for `call notification/show`.
    deskpet call <METHOD> [-p JSON] [-q] [--json]
                                  Invoke any RPC method (NDJSON).

NETWORK:
    NDJSON RPC + Swagger UI       See log on startup for ports.
                                  NDJSON: 127.0.0.1:${DESKPET_RPC_PORT:-47800}
                                  HTTP:   http://127.0.0.1:${DESKPET_HTTP_PORT:-47801}/

ENV:
    DESKPET_RPC_PORT              Override NDJSON RPC port.
    DESKPET_HTTP_PORT             Override HTTP RPC port.
    DESKPET_PORT                  Legacy alias for DESKPET_RPC_PORT (send CLI).

See docs/rpc.md for the full RPC method catalog.
";

fn print_help() {
    println!("{HELP}");
}

/// CLI 子命令：`send` / `call` / `help`。返回 `Some(exit_code)` 表示已处理并应退出。
pub fn try_cli(args: &[String]) -> Option<i32> {
    match args.first().map(String::as_str) {
        Some("send") => Some(crate::rpc::cli::send_cli(args.get(1..).unwrap_or(&[]))),
        Some("call") => Some(crate::rpc::cli::call_cli(args.get(1..).unwrap_or(&[]))),
        Some("help") | Some("--help") | Some("-h") => {
            print_help();
            Some(0)
        }
        _ => None,
    }
}

/// 启动参数（starlink-dev serve 等同进程嵌入时可定制）。
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// `assets/` 根目录；`None` 时走 [`crate::assets_path::resolve_assets_dir`].
    pub assets_dir: Option<std::path::PathBuf>,
    /// 是否起 loopback NDJSON/HTTP RPC（同进程 notify 不依赖 TCP，但外部 CLI 需要）。
    pub start_rpc_listeners: bool,
    /// 启动时直接显示萌宠窗口（默认隐藏，靠菜单栏图标显隐）。
    pub start_visible: bool,
    /// starlink-dev `serve` 写入 pid 文件路径；退出时删除，避免残留锁。
    pub serve_pid_file: Option<std::path::PathBuf>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            assets_dir: None,
            start_rpc_listeners: true,
            start_visible: false,
            serve_pid_file: None,
        }
    }
}

/// 立即结束进程（跳过 Bevy/winit 慢速 teardown，避免主线程卡住）。
pub(crate) fn shutdown_now() -> ! {
    log::info!("deskpet: shutting down");
    if let Some(path) = LAUNCH_PID_FILE.get() {
        let _ = std::fs::remove_file(path);
    }
    std::process::exit(0);
}

/// RPC `pet/control quit`：等 drain 发出 JSON 回复后再退出。
pub(crate) fn shutdown_after_rpc() {
    std::thread::Builder::new()
        .name("deskpet-exit".into())
        .spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            shutdown_now();
        })
        .ok();
}

static LAUNCH_PID_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 启动萌宠 GUI + 可选 RPC 监听。
pub fn launch_with_inbox(rpc_inbox: RpcTaskInbox, config: LaunchConfig) -> ! {
    if let Some(path) = config.serve_pid_file {
        let _ = LAUNCH_PID_FILE.set(path);
    }
    if config.start_rpc_listeners {
        try_spawn_rpc_listeners(&rpc_inbox);
    }

    let assets_dir = config
        .assets_dir
        .clone()
        .unwrap_or_else(crate::assets_path::resolve_assets_dir);
    log::info!(
        "deskpet: assets → {}",
        crate::assets_path::assets_dir_display(&assets_dir)
    );

    apply_macos_accessory_early();

    let start_visible = config.start_visible;

    App::new()
        // Fully transparent background — only the mascot is painted.
        .insert_resource(ClearColor(Color::NONE))
        // Event-driven loop: animate at ~60 fps when focused, throttle to
        // ~30 fps otherwise. This is what keeps an idle mascot cheap.
        // Start active; `adaptive_frame_rate` rewrites this every tick.
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(ACTIVE_WAIT),
            unfocused_mode: UpdateMode::reactive(ACTIVE_WAIT),
        })
        .init_resource::<Screen>()
        .init_resource::<PetWin>()
        .init_resource::<Walk>()
        .init_resource::<Drag>()
        .init_resource::<Hover>()
        .init_resource::<Settings>()
        .init_resource::<Mascot>()
        .init_resource::<NotifyState>()
        .insert_resource(DeskpetAssetsDir(assets_dir.clone()))
        // RPC: register every method + share the inbox the listener threads
        // are pushing into. The Bevy drain system below pulls requests out
        // and dispatches them with full World access.
        .insert_resource(crate::rpc_methods::register_all())
        .insert_resource(rpc_inbox)
        .add_plugins(
            DefaultPlugins
                .build()
                .disable::<LogPlugin>()
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "deskpet".into(),
                        resolution: WindowResolution::new(WIN_W as u32, WIN_H as u32),
                        // Start hidden unless --show: lives in menu bar; click tray icon to toggle.
                        visible: start_visible,
                        transparent: true,
                        // Required for a transparent window to actually show
                        // through on macOS (and the safe choice on Windows);
                        // without it the 3D background composites to black.
                        composite_alpha_mode: CompositeAlphaMode::PostMultiplied,
                        decorations: false,
                        resizable: false,
                        has_shadow: false,
                        window_level: WindowLevel::AlwaysOnTop,
                        position: WindowPosition::Centered(MonitorSelection::Primary),
                        ..default()
                    }),
                    ..default()
                })
                // Pixel-art friendly default sampling for any future sprites.
                .set(ImagePlugin::default_nearest())
                // Resolve `assets/` relative to the *current working directory*
                // so it works whether launched via `cargo run` (manifest dir)
                // or directly as `./target/debug/deskpet` (exe dir would
                // otherwise be used, missing the assets).
                .set(AssetPlugin {
                    file_path: assets_dir.to_string_lossy().into_owned(),
                    ..default()
                }),
        )
        .add_plugins(EguiPlugin::default())
        .add_plugins(TrayIconPlugin)
        // OS-level desktop notifications (Notification Center / Action Center).
        // Noop backend unless built with `--features os-notify`; see
        // src/os_notify/mod.rs for the feature/target matrix.
        .add_plugins(OsNotifyPlugin {
            notifier: make_notifier(),
        })
        .add_systems(Startup, (spawn_scene, create_tray))
        .add_systems(
            Update,
            (tray_menu, tray_clicks, apply_platform_tweaks),
        )
        .add_systems(
            Update,
            (
                init_screen,
                crate::rpc::bevy_bridge::drain_rpc_tasks, // exclusive: dispatches RPC with World access
                advance_notify, // promote queued reminders, expire the current
                drive_input,    // global cursor: hit_test + drag + click + quit
                focus_on_hover, // activate window when hovered so egui gets events
                consume_hop,    // HUD "Hop" button -> jump
                walk,           // idle random walk
                apply_window_pos,
                animate,
                adaptive_frame_rate, // lazy render: throttle when idle
            )
                .chain(),
        )
        // GLB：挂动画图；idle / walk 由 sync_mascot_visual 驱动；Toon 材质由 pet_render 处理。
        .add_systems(
            Update,
            (
                setup_mascot_animation,
                sync_mascot_visual,
                crate::pet_render::apply_bright_materials,
                crate::pet_render::sync_material_tuning,
            )
                .chain(),
        )
        // egui HUD + reminder bubble are drawn in the egui pass on the primary
        // window's context.
        .add_systems(EguiPrimaryContextPass, (notify_bubble, hud_system).chain())
        .run();
    std::process::exit(0);
}

/// Bevy 资源路径（托盘/GLB 等）。
#[derive(Resource, Clone)]
pub(crate) struct DeskpetAssetsDir(pub std::path::PathBuf);

/// macOS：在 winit 初始化前设为 Accessory，否则菜单栏图标可能不出现。
fn apply_macos_accessory_early() {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm)
                .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            log::info!("deskpet: macOS Accessory（菜单栏模式）已提前启用");
        }
    }
}

fn try_spawn_rpc_listeners(rpc_inbox: &RpcTaskInbox) {
    if let Err(e) = crate::rpc::server::spawn_listener(rpc_inbox.clone()) {
        let hint = if e.kind() == std::io::ErrorKind::AddrInUse {
            "（端口已被占用：可能已有 deskpet 在跑；同进程 notify 仍可用，外部 CLI 请连已有实例或先退出旧进程）"
        } else {
            ""
        };
        log::warn!("deskpet: NDJSON RPC 未启动: {e}{hint}");
    }
    if let Err(e) = crate::rpc::http::spawn_server(rpc_inbox.clone()) {
        let hint = if e.to_string().contains("Address already in use")
            || e.to_string().contains("addr in use")
        {
            "（47801 已被占用，Swagger/HTTP 调用不可用）"
        } else {
            ""
        };
        log::warn!("deskpet: HTTP RPC 未启动: {e}{hint}");
    }
}

// ---- Resources -------------------------------------------------------------

/// Desktop geometry of the monitor the mascot lives on (physical pixels).
#[derive(Resource, Default)]
pub(crate) struct Screen {
    ready: bool,
    pub(crate) origin: Vec2,
    pub(crate) size: Vec2,
    scale: f32,
}

/// Logical top-left position of the window on the desktop (physical pixels).
#[derive(Resource, Default)]
pub(crate) struct PetWin {
    pub(crate) pos: Vec2,
}

/// Idle wander state machine.
#[derive(Resource)]
pub(crate) struct Walk {
    pub(crate) target_x: f32,
    pub(crate) moving: bool,
    pub(crate) wait: Timer,
}

impl Default for Walk {
    fn default() -> Self {
        Self {
            target_x: 0.0,
            moving: false,
            wait: Timer::from_seconds(1.5, TimerMode::Once),
        }
    }
}

/// Drag state. The position comes from the OS-global pointer (so it survives
/// passthrough toggling and the pointer leaving the window rect); the button
/// state comes from Bevy (whenever a click matters, `hit_test` is on).
#[derive(Resource, Default)]
struct Drag {
    active: bool,
    offset: Vec2,
}

/// Where the global pointer sits relative to the window, used for hit-testing,
/// dragging, and the adaptive frame rate.
#[derive(Resource, Default)]
struct Hover {
    /// Pointer is over an interactive region (mascot body OR HUD panel). This
    /// is what `hit_test` follows.
    inside: bool,
    /// Pointer is over the grabbable mascot body specifically. Only the body
    /// starts a window drag — the HUD belongs to egui.
    on_body: bool,
    /// Pointer is approaching the window (within `NEAR_R`) or over the HUD.
    near: bool,
}

/// User-tweakable settings, shared between the HUD and the behavior systems.
#[derive(Resource)]
pub(crate) struct Settings {
    pub(crate) walk_speed: f32,
    pub(crate) wander: bool,
    /// Set by the HUD "Hop" button; consumed by `consume_hop`.
    pub(crate) hop_request: bool,
    /// HUD collapsed (just a gear) vs expanded (full panel).
    pub(crate) hud_open: bool,
    /// 是否显示萌宠（关 = 隐藏 3D 模型；开 = idle / 行走动画）.
    pub(crate) show_animation: bool,
    /// 卡通 Toon 渲染（关 = 柔和 PBR；开 = 色阶量化）.
    pub(crate) toon_shading: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            walk_speed: WALK_SPEED,
            wander: true,
            hop_request: false,
            hud_open: false,
            show_animation: true,
            toon_shading: false,
        }
    }
}

// ---- Components ------------------------------------------------------------

#[derive(Component)]
pub(crate) struct Pet {
    vy: f32,
    /// -1.0 faces left, +1.0 faces right (lerped into a Y-axis yaw).
    facing: f32,
    yaw: f32,
    breathe: f32,
    blink: Timer,
    blink_amount: f32,
}

impl Default for Pet {
    fn default() -> Self {
        Self {
            vy: 0.0,
            facing: 1.0,
            yaw: 0.0,
            breathe: 0.0,
            blink: Timer::from_seconds(3.5, TimerMode::Repeating),
            blink_amount: 1.0,
        }
    }
}

#[derive(Component)]
struct Eye;

/// The 3D camera, so we can constrain its viewport to the left pet sub-area.
#[derive(Component)]
struct PetCamera;

/// Which mascot is in use this run.
#[derive(Resource, Default)]
pub(crate) struct Mascot {
    /// True when a rigged GLB model was loaded instead of the procedural slime.
    pub(crate) use_glb: bool,
    /// Currently loaded GLB filename (under `assets/`); toggled by the HUD.
    pub(crate) glb: String,
}

/// Carries the loaded GLB's animation graph + node so `setup_mascot_animation`
/// can start it once the scene's `AnimationPlayer` spawns in.
#[derive(Component)]
struct PetAnim {
    graph: Handle<AnimationGraph>,
    idle_node: AnimationNodeIndex,
    walk_node: AnimationNodeIndex,
}

// ---- Startup ---------------------------------------------------------------

fn spawn_scene(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut mascot: ResMut<Mascot>,
    assets_dir: Res<DeskpetAssetsDir>,
) {
    let use_glb = mascot_glb_exists(&assets_dir.0);
    mascot.use_glb = use_glb;
    if use_glb {
        log::info!(
            "deskpet: 加载 GLB 萌宠 {} + 行走 {}",
            MASCOT_GLB,
            MASCOT_WALK_GLB
        );
    } else {
        let dir = &assets_dir.0;
        log::warn!(
            "deskpet: 未找到萌宠 GLB（需要 {}/{} 与 {}/{}），回退程序化史莱姆。\
             assets 目录: {}；请 `just install_loc` 后 `starlink-serve restart --show`",
            dir.display(),
            MASCOT_GLB,
            dir.display(),
            MASCOT_WALK_GLB,
            crate::assets_path::assets_dir_display(dir),
        );
    }

    // Camera framing differs: the rigged GLB stands ~1.7m tall on y=0, the
    // procedural slime is a unit sphere at the origin. The camera is shifted
    // sideways (look-x = cam-x) so the mascot at world x=0 lands in the left
    // pet area (~24% across the window), leaving the right side for the HUD.
    let cam_tf = if use_glb {
        Transform::from_xyz(1.03, 0.95, 3.1).looking_at(Vec3::new(1.03, 0.9, 0.0), Vec3::Y)
    } else {
        Transform::from_xyz(1.19, 0.18, 3.6).looking_at(Vec3::new(1.19, 0.05, 0.0), Vec3::Y)
    };
    commands.spawn((
        PetCamera,
        Camera3d::default(),
        cam_tf,
        Msaa::Off,
        Tonemapping::AcesFitted,
        Exposure { ev100: 12.5 },
        AmbientLight {
            color: Color::srgb(0.96, 0.98, 1.0),
            brightness: 2_200.0,
            affects_lightmapped_meshes: true,
        },
    ));

    crate::pet_render::setup_studio_lighting(&mut commands);

    if use_glb {
        mascot.glb = MASCOT_GLB.to_string();
        spawn_glb_pet(&mut commands, &asset_server, &mut graphs);
        return;
    }

    // ---- Fallback: procedural slime --------------------------------------
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.76, 1.0),
        perceptual_roughness: 0.35,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.09, 0.16),
        perceptual_roughness: 0.4,
        ..default()
    });
    let shine = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        emissive: LinearRgba::rgb(1.0, 1.0, 1.0),
        ..default()
    });

    let body_mesh = meshes.add(Sphere::new(BODY_W));
    let eye_mesh = meshes.add(Sphere::new(0.17));
    let shine_mesh = meshes.add(Sphere::new(0.055));
    let mouth_mesh = meshes.add(Sphere::new(0.1));

    commands
        .spawn((
            Pet::default(),
            Transform::from_xyz(0.0, 0.0, 0.0),
            Visibility::Visible,
        ))
        .with_children(|p| {
            // Body (root squash handles the breathing/jump deformation).
            p.spawn((Mesh3d(body_mesh), MeshMaterial3d(body.clone())));

            // Eyes, sitting on the front of the sphere facing the camera.
            for sx in [-0.34_f32, 0.34] {
                p.spawn((
                    Eye,
                    Mesh3d(eye_mesh.clone()),
                    MeshMaterial3d(dark.clone()),
                    Transform::from_xyz(sx, 0.16, 0.86),
                ))
                .with_children(|e| {
                    e.spawn((
                        Mesh3d(shine_mesh.clone()),
                        MeshMaterial3d(shine.clone()),
                        Transform::from_xyz(0.06, 0.06, 0.12),
                    ));
                });
            }

            // Mouth: a small flattened dark sphere.
            p.spawn((
                Mesh3d(mouth_mesh),
                MeshMaterial3d(dark.clone()),
                Transform::from_xyz(0.0, -0.12, 0.92).with_scale(Vec3::new(1.6, 0.7, 0.6)),
            ));
        });
}

/// Spawn the single mascot GLB; walk clip comes from [`MASCOT_WALK_GLB`].
fn spawn_glb_pet(
    commands: &mut Commands,
    asset_server: &AssetServer,
    graphs: &mut Assets<AnimationGraph>,
) {
    let scene = asset_server.load(GltfAssetLabel::Scene(0).from_asset(MASCOT_GLB));
    let idle_clip = asset_server.load(GltfAssetLabel::Animation(0).from_asset(MASCOT_GLB));
    let walk_clip = asset_server.load(GltfAssetLabel::Animation(0).from_asset(MASCOT_WALK_GLB));
    let (graph, nodes) = AnimationGraph::from_clips([idle_clip, walk_clip]);
    let idle_node = nodes[0];
    let walk_node = nodes[1];
    commands.spawn((
        Pet::default(),
        SceneRoot(scene),
        Transform::from_xyz(0.0, 0.0, 0.0),
        Visibility::Visible,
        PetAnim {
            graph: graphs.add(graph),
            idle_node,
            walk_node,
        },
    ));
}

/// macOS: keep the app a menu-bar–only "accessory" (no Dock icon). We re-assert
/// the policy every frame because winit sets `Regular` during init / on window
/// changes and can otherwise win the race. Setting it to the same value is a
/// no-op in AppKit, so this is cheap and flicker-free. No-op off macOS.
/// `NonSendMarker` pins this system to the main thread (required for AppKit).
fn apply_platform_tweaks(_main_thread: NonSendMarker, mut logged: Local<bool>) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
        use objc2_foundation::MainThreadMarker;
        if let Some(mtm) = MainThreadMarker::new() {
            NSApplication::sharedApplication(mtm)
                .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            if !*logged {
                *logged = true;
                info!("deskpet: macOS Accessory activation policy applied (no Dock icon)");
            }
        } else if !*logged {
            *logged = true;
            warn!("deskpet: apply_platform_tweaks not on main thread; Dock icon may persist");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = &mut *logged;
    }
}

/// Attach animation graph once the GLB's `AnimationPlayer` appears.
fn setup_mascot_animation(
    mut commands: Commands,
    pet_anim: Query<&PetAnim>,
    players: Query<Entity, Added<AnimationPlayer>>,
) {
    let Ok(pa) = pet_anim.single() else {
        return;
    };
    for entity in &players {
        commands.entity(entity).insert((
            AnimationGraphHandle(pa.graph.clone()),
            AnimationTransitions::new(),
        ));
    }
}

/// HUD「显示动画」+ 行走状态：控制萌宠显隐与 idle / walk 切换。
fn sync_mascot_visual(
    settings: Res<Settings>,
    walk: Res<Walk>,
    mut pets: Query<(&PetAnim, &mut Visibility), With<Pet>>,
    mut anim: Query<(&mut AnimationTransitions, &mut AnimationPlayer)>,
    mut last_clip: Local<Option<AnimationNodeIndex>>,
) {
    let Ok((pa, mut vis)) = pets.single_mut() else {
        return;
    };

    if !settings.show_animation {
        *vis = Visibility::Hidden;
        *last_clip = None;
        return;
    }
    *vis = Visibility::Visible;

    let Ok((mut transitions, mut player)) = anim.single_mut() else {
        return;
    };
    let target = if walk.moving {
        pa.walk_node
    } else {
        pa.idle_node
    };
    if *last_clip == Some(target) {
        return;
    }
    transitions
        .play(&mut player, target, Duration::from_millis(250))
        .repeat();
    *last_clip = Some(target);
}

// ---- Systems ---------------------------------------------------------------

/// Resolve the monitor bounds once the windowing backend reports them, and
/// seat the mascot on the bottom-center of the desktop.
fn init_screen(
    mut screen: ResMut<Screen>,
    mut petwin: ResMut<PetWin>,
    mut walk: ResMut<Walk>,
    monitors: Query<&Monitor>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if screen.ready {
        return;
    }
    let Some(m) = monitors.iter().next() else {
        return; // monitors not enumerated yet; try again next frame.
    };

    // Force the window to WIN_W x WIN_H *logical* pixels regardless of the
    // display's DPI scale, so the mascot and HUD (sized in logical units) fit
    // on Retina/HiDPI screens too. The camera renders the *full* window (no
    // viewport split) — bevy_egui ties egui's screen rect to the camera's
    // viewport, so restricting it would squeeze the HUD over the mascot.
    if let Ok(mut window) = window.single_mut() {
        window.resolution.set(WIN_W, WIN_H);
    }

    let scale = m.scale_factor as f32;
    let origin = Vec2::new(m.physical_position.x as f32, m.physical_position.y as f32);
    let size = Vec2::new(m.physical_width as f32, m.physical_height as f32);
    let win_w_px = WIN_W * scale;
    let win_h_px = WIN_H * scale;

    screen.origin = origin;
    screen.size = size;
    screen.scale = scale;
    screen.ready = true;

    // Start centered on the screen so it's easy to find (and never hidden
    // behind the Dock); wandering only moves it horizontally from here.
    let start_y = origin.y + (size.y - win_h_px) * 0.5;
    petwin.pos = Vec2::new(origin.x + (size.x - win_w_px) * 0.5, start_y);
    walk.target_x = petwin.pos.x;
}

/// Drive passthrough hit-test, drag, click-to-hop, and right-click-to-quit.
///
/// The pointer *position* is read from the OS (permission-free), because once
/// `hit_test` is false the window receives no Bevy cursor events. The button
/// *state* comes from Bevy: whenever a click actually matters, the pointer is
/// over the body or HUD, so `hit_test` is on and the window gets the events.
fn drive_input(
    screen: Res<Screen>,
    settings: Res<Settings>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut petwin: ResMut<PetWin>,
    mut drag: ResMut<Drag>,
    mut hover: ResMut<Hover>,
    mut notify_state: ResMut<NotifyState>,
    mut q: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut pets: Query<&mut Pet>,
) {
    if !screen.ready {
        return;
    }
    let Ok((window, mut cursor)) = q.single_mut() else {
        return;
    };
    // Use the window's *own* scale factor for all conversions, so the math
    // matches egui and Bevy exactly (avoids monitor-vs-window scale drift).
    let scale = window.resolution.scale_factor();
    let global = global_cursor_physical(scale);

    // Pointer position within the window in logical pixels (top-left origin).
    // Prefer Bevy's reported position (exact, same source egui uses); fall back
    // to the OS-global pointer only while the window is in passthrough and thus
    // receives no Bevy cursor events.
    let bevy_cursor = window.cursor_position();
    let local = match bevy_cursor {
        Some(p) => p,
        None => match global {
            Some(g) => (g - petwin.pos) / scale,
            None => return,
        },
    };

    let dist = local.distance(pet_center());
    let on_body = dist <= HIT_R;
    // Interactive HUD region: the full right panel when expanded, or just the
    // gear button when collapsed — so the empty right area passes through.
    let in_hud = if settings.hud_open {
        // Expanded: the right-side panel occupies the rightmost HUD_W.
        local.x >= WIN_W - HUD_W && local.x <= WIN_W && local.y >= 0.0 && local.y <= WIN_H
    } else {
        // Collapsed: just the gear button at the window's top-right corner.
        local.x >= WIN_W - GEAR_W && local.x <= WIN_W && local.y >= 0.0 && local.y <= GEAR_H
    };
    // Active reminder bubble: a top strip that captures clicks (to dismiss).
    let in_bubble = notify_state.showing()
        && local.x >= 0.0
        && local.x <= WIN_W
        && local.y >= 0.0
        && local.y <= BUBBLE_H;
    let inside = on_body || in_hud || in_bubble;
    hover.on_body = on_body;
    hover.inside = inside;
    hover.near = dist <= NEAR_R || in_hud || in_bubble;

    // Intercept the mouse over the body or the HUD (or mid-drag); everywhere
    // else the click falls through to whatever is behind the window.
    let want_hit = inside || drag.active;
    if cursor.hit_test != want_hit {
        cursor.hit_test = want_hit;
    }

    // A left click on the reminder bubble dismisses it (takes priority over the
    // body drag, since the bubble overlaps the mascot's upper area).
    if in_bubble && mouse.just_pressed(MouseButton::Left) {
        notify_state.dismiss();
    } else if mouse.just_pressed(MouseButton::Left) && on_body {
        // Left press on the *body* (not the HUD) starts a window drag + hop.
        // HUD clicks are left for egui.
        if let Some(g) = global {
            drag.active = true;
            drag.offset = g - petwin.pos;
        }
        if let Ok(mut pet) = pets.single_mut() {
            pet.vy = JUMP_V;
        }
    }
    if drag.active {
        if mouse.pressed(MouseButton::Left) {
            if let Some(g) = global {
                petwin.pos = g - drag.offset;
            }
        } else {
            drag.active = false;
        }
    }

    // Right click on body -> quit. Esc also quits (once the window is focused).
    if (mouse.just_pressed(MouseButton::Right) && on_body) || keys.just_pressed(KeyCode::Escape) {
        shutdown_now();
    }
}

/// Current global cursor position in **physical** desktop pixels (matching
/// `Monitor::physical_position` and `WindowPosition::At`). Permission-free on
/// both supported platforms.
#[cfg(target_os = "macos")]
fn global_cursor_physical(scale: f32) -> Option<Vec2> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    // CGEvent location is in logical "points"; multiply by the monitor scale
    // to get physical pixels. Reading it needs no Accessibility permission.
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let p = event.location();
    Some(Vec2::new(p.x as f32 * scale, p.y as f32 * scale))
}

#[cfg(target_os = "windows")]
fn global_cursor_physical(_scale: f32) -> Option<Vec2> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
    // GetCursorPos returns physical pixels for a per-monitor-DPI-aware process
    // (winit makes the process DPI aware), so no scaling needed.
    let mut p = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut p) } == 0 {
        return None;
    }
    Some(Vec2::new(p.x as f32, p.y as f32))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn global_cursor_physical(_scale: f32) -> Option<Vec2> {
    None
}

/// GitHub fallback for the protocol docs when no local copy is found (e.g. a
/// bundled `.app` without the markdown shipped alongside).
const DOCS_URL_BASE: &str = "https://github.com/nicholasgasior/r_lit/blob/main/deskpet";

/// Open the reminder protocol reference for the user. Picks the Chinese doc
/// when the locale looks `zh-*`, else English; prefers a local copy (works from
/// the crate dir or next to the binary) and falls back to the GitHub URL. No
/// extra deps — shells out to the platform opener.
fn open_protocol_docs() {
    let zh = std::env::var("LANG")
        .or_else(|_| std::env::var("LC_ALL"))
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .map(|l| l.to_lowercase().contains("zh"))
        .unwrap_or(false);
    let file = if zh { "PROTOCOL_CN.md" } else { "PROTOCOL.md" };

    // Local candidates first: CWD (dev run / direct run from crate dir), then
    // the exe dir and a macOS-bundle Resources sibling.
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(file));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(file));
            candidates.push(dir.join("../Resources").join(file));
        }
    }
    let target = candidates
        .into_iter()
        .find(|p| p.exists())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{DOCS_URL_BASE}/{file}"));

    open_path_or_url(&target);
}

/// Hand a file path or URL to the OS's default handler (no extra deps).
fn open_path_or_url(target: &str) {
    #[cfg(target_os = "macos")]
    let res = std::process::Command::new("open").arg(target).spawn();
    #[cfg(target_os = "windows")]
    let res = std::process::Command::new("cmd")
        .args(["/C", "start", "", target])
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let res = std::process::Command::new("xdg-open").arg(target).spawn();

    match res {
        Ok(_) => info!("deskpet: opened protocol docs ({target})"),
        Err(e) => warn!("deskpet: could not open protocol docs ({target}): {e}"),
    }
}

/// Create the system-tray / macOS menu-bar icon. Left-click toggles the
/// mascot; right-click opens a Show / Hide / Quit menu.
fn create_tray(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    assets_dir: Res<DeskpetAssetsDir>,
) {
    let tray_path = assets_dir.0.join("tray.png");
    let handle = match crate::tray_icon::preload_tray_png(&tray_path, &mut images) {
        Ok(h) => {
            log::info!(
                "deskpet: 菜单栏图标已加载 {}（macOS 若在 « 溢出区请展开；左键显隐萌宠，右键菜单）",
                tray_path.display()
            );
            Some(h)
        }
        Err(e) => {
            log::warn!("deskpet: 菜单栏图标加载失败 {}: {e}", tray_path.display());
            None
        }
    };
    commands.insert_resource(TrayIcon {
        icon: handle,
        tooltip: Some("deskpet — click to show/hide".into()),
        menu: Menu::new(vec![
            MenuItem::common("show", "Show", true, None),
            MenuItem::common("hide", "Hide", true, None),
            MenuItem::separator(),
            // Entry point to the reminder protocol reference (PROTOCOL.md).
            MenuItem::common("docs", "Reminder Protocol Docs", true, None),
            MenuItem::separator(),
            MenuItem::common("quit", "Quit", true, None),
        ]),
        // Left-click should toggle the mascot, not open the menu (macOS only
        // flag); the menu is reserved for right-click.
        show_menu_on_left_click: false,
    });
}

/// Right-click tray menu actions.
fn tray_menu(
    mut reader: MessageReader<MenuMessage>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    for e in reader.read() {
        match e.id.0.as_str() {
            "show" => window.visible = true,
            "hide" => window.visible = false,
            "docs" => open_protocol_docs(),
            "quit" => shutdown_now(),
            _ => {}
        }
    }
}

/// Left-click toggles the mascot's visibility; a Windows double-click hides it.
/// `bevy_tray_icon` only bridges *menu* events into Bevy, so we poll the
/// tray-icon click channel directly here.
fn tray_clicks(mut window: Query<&mut Window, With<PrimaryWindow>>) {
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        match event {
            // Toggle on left-button release (one event per click).
            TrayIconEvent::Click {
                button: TrayMouseButton::Left,
                button_state: TrayMouseButtonState::Up,
                ..
            } => {
                window.visible = !window.visible;
            }
            // Windows-only convenience: double-click hides.
            TrayIconEvent::DoubleClick {
                button: TrayMouseButton::Left,
                ..
            } => {
                window.visible = false;
            }
            _ => {}
        }
    }
}

/// macOS (and some compositors) only deliver mouse events to the *active*
/// application's key window. A frameless always-on-top overlay launched from a
/// terminal is not active, so it never receives cursor/click events and egui
/// can't be interacted with. When the pointer moves onto the body or HUD we
/// focus the window (which activates the app on macOS), so the events start
/// flowing. `NonSendMarker` pins this system to the main thread, which is
/// required to touch the `WINIT_WINDOWS` thread-local.
fn focus_on_hover(
    _main_thread: NonSendMarker,
    hover: Res<Hover>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut was_near: Local<bool>,
    mut started: Local<bool>,
) {
    let near = hover.near;
    // Focus once at startup (so the app is active from launch), then again on
    // each rising edge of "pointer approaching" — early enough that events are
    // already flowing by the time the pointer reaches the body or gear.
    let trigger = !*started || (near && !*was_near);
    if trigger {
        if let Ok(entity) = windows.single() {
            WINIT_WINDOWS.with_borrow(|winit_windows| {
                if let Some(window) = winit_windows.get_window(entity) {
                    window.focus_window();
                }
            });
        }
    }
    *started = true;
    *was_near = near;
}

/// Idle random horizontal wander across the desktop.
fn walk(
    time: Res<Time>,
    screen: Res<Screen>,
    drag: Res<Drag>,
    hover: Res<Hover>,
    settings: Res<Settings>,
    notify_state: Res<NotifyState>,
    mut petwin: ResMut<PetWin>,
    mut walk: ResMut<Walk>,
    mut pets: Query<&mut Pet>,
) {
    // Hold still while dragging, when the pointer is near, or while a reminder
    // is showing — so you can read it and click the (otherwise moving) mascot.
    if !screen.ready || drag.active || hover.near || !settings.wander || notify_state.showing() {
        // Cancel any in-progress move so it doesn't lurch when interaction ends.
        walk.moving = false;
        return;
    }
    let dt = time.delta_secs();
    let win_px = WIN_W * screen.scale;
    let min_x = screen.origin.x;
    let max_x = screen.origin.x + screen.size.x - win_px;
    let speed = settings.walk_speed;

    if walk.moving {
        let diff = walk.target_x - petwin.pos.x;
        let dir = diff.signum();
        petwin.pos.x += dir * speed * dt;
        if let Ok(mut pet) = pets.single_mut() {
            pet.facing = if dir < 0.0 { -1.0 } else { 1.0 };
        }
        if diff.abs() < speed * dt + 1.0 {
            petwin.pos.x = walk.target_x;
            walk.moving = false;
            let idle = rand::thread_rng().gen_range(1.5..4.5);
            walk.wait = Timer::from_seconds(idle, TimerMode::Once);
        }
    } else {
        walk.wait.tick(time.delta());
        if walk.wait.is_finished() {
            walk.target_x = rand::thread_rng().gen_range(min_x..max_x.max(min_x + 1.0));
            walk.moving = true;
        }
    }
}

/// Push the logical mascot position to the actual window, clamped on-screen.
fn apply_window_pos(
    screen: Res<Screen>,
    mut petwin: ResMut<PetWin>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !screen.ready {
        return;
    }
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    let win_w_px = WIN_W * screen.scale;
    let win_h_px = WIN_H * screen.scale;
    petwin.pos.x = petwin
        .pos
        .x
        .clamp(screen.origin.x, screen.origin.x + screen.size.x - win_w_px);
    petwin.pos.y = petwin
        .pos
        .y
        .clamp(screen.origin.y, screen.origin.y + screen.size.y - win_h_px);
    window.position = WindowPosition::At(petwin.pos.as_ivec2());
}

/// Breathing squash, click hop, and blinking.
fn animate(
    time: Res<Time>,
    mut pets: Query<(&mut Pet, &mut Transform), Without<Eye>>,
    mut eyes: Query<&mut Transform, With<Eye>>,
) {
    let dt = time.delta_secs();
    let Ok((mut pet, mut tf)) = pets.single_mut() else {
        return;
    };

    // Hop physics (world-space, relative to its resting baseline at y=0).
    pet.vy -= GRAVITY * dt;
    let mut y = tf.translation.y + pet.vy * dt;
    if y <= 0.0 {
        y = 0.0;
        pet.vy = 0.0;
    }
    tf.translation.y = y;

    // Smoothly turn toward the facing direction, plus a gentle idle sway.
    pet.breathe += dt;
    let target_yaw = pet.facing.signum() * FACE_YAW;
    pet.yaw += (target_yaw - pet.yaw) * (8.0 * dt).min(1.0);
    let sway = (pet.breathe * 1.3).sin() * 0.05;
    tf.rotation = Quat::from_rotation_y(pet.yaw + sway);

    // Breathing: gentle squash/stretch; add a stretch while airborne.
    let breathe = (pet.breathe * 2.2).sin() * 0.04;
    let air = (y / 0.6).min(0.18);
    let sxz = 1.0 - breathe - air;
    let sy = 1.0 + breathe + air;
    tf.scale = Vec3::new(sxz, sy, sxz);

    // Blink: squash eyes shut briefly near the end of each cycle.
    pet.blink.tick(time.delta());
    let remaining = pet.blink.remaining_secs();
    pet.blink_amount = if remaining < 0.12 { 0.12 } else { 1.0 };
    for mut e in &mut eyes {
        e.scale.y = pet.blink_amount;
    }
}

/// Lazy rendering: pick a wait interval based on what's happening, so the app
/// only renders fast when it needs to. An untouched, resting mascot runs at the
/// idle heartbeat; approaching the pointer bumps it up; interaction/animation
/// runs at full rate.
fn adaptive_frame_rate(
    walk: Res<Walk>,
    drag: Res<Drag>,
    hover: Res<Hover>,
    notify_state: Res<NotifyState>,
    pets: Query<(&Pet, &Transform)>,
    mut winit: ResMut<WinitSettings>,
) {
    let busy = pets.iter().any(|(p, tf)| {
        tf.translation.y > 0.001 || p.vy.abs() > 0.001 || p.blink_amount < 0.999
    });
    let active = drag.active || hover.inside || walk.moving || busy || notify_state.showing();

    let wait = if active {
        ACTIVE_WAIT
    } else if hover.near {
        NEAR_WAIT
    } else {
        IDLE_WAIT
    };

    let mode = UpdateMode::reactive(wait);
    if winit.focused_mode != mode {
        winit.focused_mode = mode;
    }
    if winit.unfocused_mode != mode {
        winit.unfocused_mode = mode;
    }
}

/// Turn a HUD "Hop" request into an actual jump impulse.
fn consume_hop(mut settings: ResMut<Settings>, mut pets: Query<&mut Pet>) {
    if !settings.hop_request {
        return;
    }
    settings.hop_request = false;
    if let Ok(mut pet) = pets.single_mut() {
        pet.vy = JUMP_V;
    }
}

/// Expire the on-screen reminder when its timer runs out (or its
/// `expires_at` deadline has passed), then promote the next queued one.
/// Keeps a single reminder visible at a time, in arrival order. Also
/// drops queued notices whose deadline has passed (stale background
/// notifications).
fn advance_notify(time: Res<Time>, mut state: ResMut<NotifyState>) {
    let now = std::time::Instant::now();
    // Expire current if its TTL timer ran out OR its deadline has passed.
    let timer_done = match state.timer.as_mut() {
        Some(timer) => {
            timer.tick(time.delta());
            timer.is_finished()
        }
        None => false,
    };
    let deadline_done = state
        .current
        .as_ref()
        .map(|n| n.is_expired(now))
        .unwrap_or(false);
    if timer_done || deadline_done {
        state.dismiss();
    }
    if state.current.is_none() {
        // Drop queued notices whose deadline has passed. Loop in case
        // many are expired at once (e.g., batch background work that
        // all finished too late).
        while let Some(front) = state.queue.front() {
            if front.is_expired(now) {
                state.queue.pop_front();
            } else {
                break;
            }
        }
        if let Some(next) = state.queue.pop_front() {
            state.timer = Some(Timer::new(next.ttl, TimerMode::Once));
            state.current = Some(next);
        }
    }
}

/// Draw the active reminder as a top-anchored speech bubble over the mascot,
/// accent-colored by severity. Text wraps within the window width; a click
/// anywhere on it dismisses (handled in `drive_input` via the bubble hit zone).
fn notify_bubble(mut contexts: EguiContexts, state: Res<NotifyState>) {
    let Some(notice) = state.current.as_ref() else {
        return;
    };
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let [r, g, b] = notice.level.accent();
    let accent = egui::Color32::from_rgb(r, g, b);
    let glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 230);
    let body_col = egui::Color32::from_rgb(228, 231, 238);

    egui::Area::new(egui::Id::new("notify_bubble"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(6.0, 6.0))
        .show(ctx, |ui| {
            ui.set_max_width(WIN_W - 12.0);
            egui::Frame::new()
                .fill(glass)
                .stroke(egui::Stroke::new(1.5, accent))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(8)
                .show(ui, |ui| {
                    // Wrap text to the bubble's inner width.
                    ui.set_max_width(WIN_W - 28.0);
                    if let Some(title) = &notice.title {
                        ui.label(egui::RichText::new(title).strong().color(accent));
                    }
                    ui.label(egui::RichText::new(&notice.body).color(body_col));
                });
        });
}

/// The egui HUD, drawn over the same (transparent) window as the mascot, in
/// `EguiPrimaryContextPass`. Collapsed by default to just a gear button so it
/// doesn't obstruct the view; click the gear to expand a semi-transparent
/// side panel. The hit-test region in `drive_input` tracks this open/closed
/// state so the empty area always passes clicks through.
fn hud_system(
    mut contexts: EguiContexts,
    mut settings: ResMut<Settings>,
    mut styled: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Scale all egui text/widgets by UI_SCALE once. We do this via the style
    // (not zoom_factor) on purpose: zoom changes egui's coordinate space and
    // would desync from the hit-test regions we compute in logical pixels.
    if !*styled {
        *styled = true;
        let mut style = (*ctx.style()).clone();
        for (_text_style, font_id) in style.text_styles.iter_mut() {
            font_id.size *= UI_SCALE;
        }
        style.spacing.button_padding = style.spacing.button_padding * UI_SCALE;
        style.spacing.interact_size.y *= UI_SCALE;
        style.spacing.item_spacing = style.spacing.item_spacing * UI_SCALE;
        ctx.set_style(style);
    }

    // Semi-transparent dark fill so the HUD blends with the desktop behind it.
    let glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 205);

    if !settings.hud_open {
        egui::Area::new(egui::Id::new("hud_gear"))
            .fixed_pos(egui::pos2(WIN_W - GEAR_W + 4.0, 4.0))
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(glass)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(4)
                    .show(ui, |ui| {
                        if ui.button("⚙").on_hover_text("Open HUD").clicked() {
                            settings.hud_open = true;
                        }
                    });
            });
        return;
    }

    let style = ctx.style();
    let frame = egui::Frame::side_top_panel(&style).fill(glass);
    egui::SidePanel::right("hud")
        .exact_width(HUD_W)
        .resizable(false)
        .frame(frame)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("deskpet");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("–").on_hover_text("Collapse").clicked() {
                        settings.hud_open = false;
                    }
                });
            });
            ui.add(egui::Slider::new(&mut settings.walk_speed, 0.0..=300.0).text("speed"));
            ui.checkbox(&mut settings.wander, "Wander");
            ui.checkbox(
                &mut settings.show_animation,
                "显示动画（关则隐藏桌面萌宠）",
            );
            ui.checkbox(&mut settings.toon_shading, "柔和提亮（实验）");
            ui.horizontal(|ui| {
                if ui.button("Hop").clicked() {
                    settings.hop_request = true;
                }
            });
            if ui
                .button("Protocol Docs")
                .on_hover_text("Open the reminder protocol reference")
                .clicked()
            {
                open_protocol_docs();
            }
            if ui
                .add(egui::Button::new("Quit").fill(egui::Color32::from_rgb(120, 40, 40)))
                .clicked()
            {
                shutdown_now();
            }
        });
}
