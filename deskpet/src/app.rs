//! Bevy 桌面宠物应用体 — 由 `lib::launch_with_inbox` 启动。

use std::time::Duration;

use bevy::prelude::*;
use bevy::asset::AssetEventSystems;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::window::{
    CompositeAlphaMode, CursorOptions, ExitCondition, Monitor, PrimaryWindow, WindowLevel,
    WindowPosition, WindowResolution,
};
use bevy::asset::AssetPlugin;
use bevy::ecs::system::NonSendMarker;
use bevy::gltf::GltfAssetLabel;
use bevy::winit::{UpdateMode, WinitSettings, WINIT_WINDOWS};
use bevy_egui::{
    egui, input::EguiWantsInput, EguiContexts, EguiPlugin, EguiPostUpdateSet,
    EguiPrimaryContextPass,
};
use bevy_tray_icon::plugin::menu_event::MenuMessage;
use bevy_tray_icon::plugin::TrayIconPlugin;
use bevy_tray_icon::resource::{Menu, MenuIconKind, MenuItem, TrayIcon};
use bevy_tray_icon::{
    MouseButton as TrayMouseButton, MouseButtonState as TrayMouseButtonState, TrayIconEvent,
};
use rand::Rng;

use crate::notify::NotifyState;
use crate::os_notify::{OsNotifyPlugin, make_notifier};
use crate::rpc::bevy_bridge::RpcTaskInbox;
use bevy::log::LogPlugin;
use bevy_spine::{SpinePlugin, SpineSet};

// ============================================================================
//  SIZE / LAYOUT KNOBS — tune these to taste while debugging the pet.
// ============================================================================
//
// The window is partitioned: a pet area on the left holds the 3D mascot, and
// an egui HUD panel sits on the right. The whole window is one rigid body —
// dragging the mascot moves the HUD with it.

/// Visible mascot area width (logical px). Shrink this to shrink the slime.
pub(crate) const PET_W: f32 = 150.0;
/// Window height (logical px).
pub(crate) const WIN_H: f32 = 200.0;
/// Expanded HUD panel width (logical px). Spine 动画名需稍宽。
const HUD_W: f32 = 172.0;
/// HUD widget/text size multiplier.
const UI_SCALE: f32 = 1.2;
// ============================================================================

/// Total logical window dimensions (pixels).
pub(crate) const WIN_W: f32 = PET_W + HUD_W;
/// Mascot grab radius (logical px), centered on the window where the camera
/// frames the mascot.
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

/// ~30 fps while moving / dragging / hovered / animating (省 CPU/GPU).
const ACTIVE_WAIT: Duration = Duration::from_millis(33);
/// ~30 fps when the pointer is approaching the window (snappy to grab).
const NEAR_WAIT: Duration = Duration::from_millis(33);
/// ~8 fps idle heartbeat: just enough to poll the global cursor for hover and
/// advance the blink timer. This is the floor cost of a perched mascot.
const IDLE_WAIT: Duration = Duration::from_millis(120);
/// Screen-space radius (logical px) around the pet center within which the
/// pointer counts as "approaching".
const NEAR_R: f32 = PET_W * 1.4;

const GEAR_W: f32 = 30.0 * UI_SCALE;
const GEAR_H: f32 = 30.0 * UI_SCALE;
const BUBBLE_H: f32 = WIN_H * 0.5;

/// Logical center of the mascot body within the window (top-left origin): left pet area.
fn pet_center() -> Vec2 {
    Vec2::new(PET_W * 0.5, WIN_H * 0.5)
}

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

/// 菜单栏托盘行为模式。
#[derive(Debug, Clone)]
pub enum TrayMode {
    /// deskpet 默认：左键显隐萌宠，右键 Show/Hide/Docs/Quit。
    Deskpet,
    /// starlink-dev serve：RMB 风格菜单栏，左键弹出菜单。
    StarlinkDev {
        /// admin HTTP 实际监听端口写入此文件（如 `8901` 一行）。
        admin_port_file: std::path::PathBuf,
    },
}

impl Default for TrayMode {
    fn default() -> Self {
        Self::Deskpet
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
    /// `notification/show` 时是否自动弹出萌宠窗口（默认 false，仅手动显示）。
    pub auto_show_on_notify: bool,
    /// starlink-dev `serve` 写入 pid 文件路径；退出时删除，避免残留锁。
    pub serve_pid_file: Option<std::path::PathBuf>,
    /// 菜单栏图标与菜单行为。
    pub tray_mode: TrayMode,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            assets_dir: None,
            start_rpc_listeners: true,
            start_visible: false,
            auto_show_on_notify: false,
            serve_pid_file: None,
            tray_mode: TrayMode::default(),
        }
    }
}

#[derive(Resource, Clone)]
pub(crate) struct LaunchTrayConfig {
    pub(crate) mode: TrayMode,
    pub(crate) start_visible: bool,
    pub(crate) auto_show_on_notify: bool,
}

/// 立即结束进程（跳过 Bevy/winit 慢速 teardown，避免主线程卡住）。
fn append_serve_exit_log(line: &str) {
    let Some(path) = LAUNCH_PID_FILE.get() else {
        return;
    };
    let log = path
        .parent()
        .and_then(|p| p.parent())
        .map(|h| h.join("log/serve.log"));
    if let Some(log) = log {
        if let Some(parent) = log.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
        {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}

pub(crate) fn shutdown_now() -> ! {
    log::info!("deskpet: shutting down");
    if LAUNCH_PID_FILE.get().is_some() {
        append_serve_exit_log(&format!(
            "[serve] deskpet 正常退出 pid={}",
            std::process::id()
        ));
    }
    if let Some(path) = LAUNCH_PID_FILE.get() {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = LAUNCH_ADMIN_PORT_FILE.get() {
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
static LAUNCH_ADMIN_PORT_FILE: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 启动萌宠 GUI + 可选 RPC 监听。
pub fn launch_with_inbox(rpc_inbox: RpcTaskInbox, config: LaunchConfig) -> ! {
    if let Some(path) = config.serve_pid_file {
        let _ = LAUNCH_PID_FILE.set(path);
    }
    if let TrayMode::StarlinkDev { admin_port_file } = &config.tray_mode {
        let _ = LAUNCH_ADMIN_PORT_FILE.set(admin_port_file.clone());
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
    let tray_config = LaunchTrayConfig {
        mode: config.tray_mode.clone(),
        start_visible,
        auto_show_on_notify: config.auto_show_on_notify,
    };
    // starlink-dev 菜单栏模式：进程生命周期只由菜单 Quit 决定；宠物窗口的
    // 显隐/关闭都不应退出进程（否则双击/右键隐藏会连带杀掉菜单栏图标）。
    // 桌面模式保持原默认（所有窗口关闭即退出）。
    let tray_exit_condition = match &tray_config.mode {
        TrayMode::StarlinkDev { .. } => ExitCondition::DontExit,
        TrayMode::Deskpet => ExitCondition::OnAllClosed,
    };

    let use_spine = crate::spine_pet::spine_enabled(&assets_dir);
    if use_spine {
        log::info!("deskpet: 启动 Spine 模式（mascot.json 内 6 序列，HUD 选播）");
    }
    let (init_win_w, init_win_h) = if use_spine {
        (
            crate::spine_pet::SPINE_WIN_W as u32,
            crate::spine_pet::SPINE_WIN_H as u32,
        )
    } else {
        (WIN_W as u32, WIN_H as u32)
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::NONE))
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
        .init_resource::<crate::spine_pet::SpineHud>()
        .init_resource::<NotifyState>()
        .insert_resource(DeskpetAssetsDir(assets_dir.clone()))
        .insert_resource(tray_config)
        .insert_resource(crate::rpc_methods::register_all())
        .insert_resource(rpc_inbox)
        .add_plugins(
            DefaultPlugins
                .build()
                .disable::<LogPlugin>()
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "deskpet".into(),
                        resolution: WindowResolution::new(init_win_w, init_win_h),
                        visible: start_visible,
                        transparent: true,
                        composite_alpha_mode: CompositeAlphaMode::PostMultiplied,
                        decorations: false,
                        resizable: false,
                        has_shadow: false,
                        window_level: WindowLevel::AlwaysOnTop,
                        position: WindowPosition::Centered(MonitorSelection::Primary),
                        ..default()
                    }),
                    exit_condition: tray_exit_condition,
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(AssetPlugin {
                    file_path: assets_dir.to_string_lossy().into_owned(),
                    ..default()
                }),
        )
        .add_plugins(TrayIconPlugin)
        .add_plugins(EguiPlugin::default())
        .add_plugins(OsNotifyPlugin {
            notifier: make_notifier(),
        });
    if use_spine {
        app.add_plugins(SpinePlugin)
            .add_systems(
                Update,
                (
                    crate::spine_pet::on_spine_ready.in_set(SpineSet::OnReady),
                    crate::spine_pet::apply_spine_hud_requests.in_set(SpineSet::OnReady),
                    crate::spine_pet::advance_spine_auto_play.in_set(SpineSet::OnEvent),
                    crate::spine_pet::sync_spine_visual,
                    crate::spine_pet::log_spine_draw_ready,
                    crate::spine_pet::log_spine_layout_once,
                    crate::spine_pet::sync_spine_window_layout,
                    crate::spine_pet::sync_spine_pet_camera_viewport,
                ),
            );
    } else {
        app.add_systems(
            Update,
            (
                crate::pet_render::patch_gltf_materials,
                setup_mascot_animation,
                sync_mascot_visual,
                crate::pet_render::apply_bright_materials,
                crate::pet_render::sync_material_tuning,
                crate::pet_render::sync_toon_camera,
            )
                .chain()
                .after(AssetEventSystems),
        );
    }
    app.init_resource::<PetClickTracker>()
        .add_systems(
            Startup,
            (
                spawn_scene,
                create_tray,
                crate::pet_render::init_render_resources,
            ),
        )
        .add_systems(
            Update,
            (
                tray_menu,
                sync_starlink_tray_pet_check,
                tray_clicks,
                apply_platform_tweaks,
            ),
        )
        .add_systems(
            Update,
            (
                init_screen,
                crate::rpc::bevy_bridge::drain_rpc_tasks, // exclusive: dispatches RPC with World access
                advance_notify, // promote queued reminders, expire the current
                drive_input,    // global cursor: hit_test + hover + drag move
                focus_on_hover, // activate window when hovered so mouse events flow
                consume_hop,    // RPC "Hop" action -> jump
                walk,           // idle random walk
                apply_window_pos,
                animate,
                adaptive_frame_rate, // lazy render: throttle when idle
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, (notify_bubble, hud_system).chain())
        // 宠物点击/跳跃须在 egui 之后，并用 EguiWantsInput 避免与 HUD 争抢。
        .add_systems(
            PostUpdate,
            drive_pet_clicks.after(EguiPostUpdateSet::ProcessOutput),
        );
    app.run();
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
    /// 窗口应接收鼠标（body / HUD / 气泡），用于 `hit_test` 与 passthrough。
    inside: bool,
    /// 指针在可拖拽的萌宠身体上（不含 HUD 条带）。
    on_body: bool,
    /// 指针在 HUD 条带（展开侧栏或 ⚙ 区）。
    in_hud: bool,
    /// 指针在通知气泡区。
    in_bubble: bool,
    /// Pointer is approaching the window (within `NEAR_R`).
    near: bool,
}

/// User-tweakable settings, shared between the HUD and the behavior systems.
#[derive(Resource)]
pub(crate) struct Settings {
    pub(crate) walk_speed: f32,
    pub(crate) wander: bool,
    /// Set by RPC `pet/control` Hop action; consumed by `consume_hop`.
    pub(crate) hop_request: bool,
    /// 是否显示萌宠（关 = 隐藏 3D 模型；开 = idle / 行走动画）.
    pub(crate) show_animation: bool,
    /// HUD 侧栏是否展开（折叠时仅显示 ⚙）。
    pub(crate) hud_open: bool,
    /// Toon 动漫渲染（开 = Unlit 烘焙色 + 描边 mesh；关 = 柔和 PBR 实验）.
    pub(crate) toon_shading: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            walk_speed: WALK_SPEED,
            wander: true,
            hop_request: false,
            show_animation: true,
            hud_open: false,
            toon_shading: true,
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

/// The 3D camera marker.
#[derive(Component)]
pub(crate) struct PetCamera;

/// Which mascot is in use this run.
#[derive(Resource, Default)]
pub(crate) struct Mascot {
    /// True when a rigged GLB model was loaded instead of the procedural slime.
    pub(crate) use_glb: bool,
    /// True when Spine 2D rig replaces GLB (`DESKPET_SPINE=1`).
    pub(crate) use_spine: bool,
    /// Currently loaded GLB filename (under `assets/`); switched via RPC `set_glb`.
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
    skeletons: Option<ResMut<Assets<bevy_spine::SkeletonData>>>,
    mut mascot: ResMut<Mascot>,
    mut settings: ResMut<Settings>,
    assets_dir: Res<DeskpetAssetsDir>,
) {
    let use_spine = crate::spine_pet::spine_enabled(&assets_dir.0);
    mascot.use_spine = use_spine;
    if use_spine {
        settings.hud_open = false;
        settings.show_animation = true;
        settings.wander = false;
        log::info!(
            "deskpet: Spine — {}（单文件 6 序列，HUD 选播）",
            crate::spine_pet::SPINE_JSON,
        );
        mascot.use_glb = false;
        let Some(mut skeletons) = skeletons else {
            log::error!("deskpet: SpinePlugin 未加载，无法 spawn Spine 萌宠");
            return;
        };
        crate::spine_pet::spawn_spine_camera(&mut commands);
        crate::spine_pet::spawn_spine_pet(&mut commands, &asset_server, &mut skeletons);
        return;
    }

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
    // procedural slime is a unit sphere at the origin. The mascot at world x=0
    // lands at the window center (the window *is* the pet area).
    let cam_tf = if use_glb {
        Transform::from_xyz(0.0, 0.95, 3.1).looking_at(Vec3::new(0.0, 0.9, 0.0), Vec3::Y)
    } else {
        Transform::from_xyz(0.0, 0.18, 3.6).looking_at(Vec3::new(0.0, 0.05, 0.0), Vec3::Y)
    };
    commands.spawn((
        PetCamera,
        Camera3d::default(),
        cam_tf,
        Msaa::Off,
        Tonemapping::None,
        Exposure { ev100: 0.0 },
        AmbientLight {
            color: Color::srgb(0.96, 0.98, 1.0),
            brightness: 0.0,
            affects_lightmapped_meshes: true,
        },
    ));

    crate::pet_render::setup_studio_lighting(&mut commands);

    if use_glb {
        mascot.glb = MASCOT_GLB.to_string();
        spawn_glb_pet(&mut commands, &asset_server, &mut graphs);
        return;
    }

    // ---- Fallback: procedural slime (Toon Unlit，与 GLB 管线一致) ------
    let body = materials.add(StandardMaterial {
        base_color: Color::srgb(0.46, 0.76, 1.0),
        unlit: true,
        ..default()
    });
    let dark = materials.add(StandardMaterial {
        base_color: Color::srgb(0.015, 0.015, 0.025),
        unlit: true,
        ..default()
    });
    let shine = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
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
    mascot: Res<Mascot>,
    mut commands: Commands,
    pet_anim: Query<&PetAnim>,
    players: Query<Entity, Added<AnimationPlayer>>,
) {
    if mascot.use_spine {
        return;
    }
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
    mascot: Res<Mascot>,
    settings: Res<Settings>,
    walk: Res<Walk>,
    mut pets: Query<(&PetAnim, &mut Visibility), With<Pet>>,
    mut anim: Query<(&mut AnimationTransitions, &mut AnimationPlayer)>,
    mut last_clip: Local<Option<AnimationNodeIndex>>,
) {
    if mascot.use_spine {
        return;
    }
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
    mascot: Res<Mascot>,
    settings: Res<Settings>,
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

    let win_w = if mascot.use_spine {
        crate::spine_pet::SPINE_WIN_W
    } else {
        WIN_W
    };
    let win_h = if mascot.use_spine {
        crate::spine_pet::SPINE_WIN_H
    } else {
        WIN_H
    };

    if let Ok(mut window) = window.single_mut() {
        if !mascot.use_spine {
            window.resolution.set(win_w, win_h);
        } else {
            window.resolution.set(crate::spine_pet::SPINE_WIN_W, crate::spine_pet::SPINE_WIN_H);
        }
    }

    let scale = m.scale_factor as f32;
    let origin = Vec2::new(m.physical_position.x as f32, m.physical_position.y as f32);
    let size = Vec2::new(m.physical_width as f32, m.physical_height as f32);
    let win_w_px = win_w * scale;
    let win_h_px = win_h * scale;

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

/// 萌宠左键连击检测（starlink-dev：双击隐藏）。
#[derive(Resource, Default)]
struct PetClickTracker {
    last_at: Option<std::time::Instant>,
    last_on_body: bool,
}

const PET_DOUBLE_CLICK_MS: u128 = 450;

/// 每帧更新指针分区与 passthrough；拖拽进行中也在此推进。
/// 左键/右键对萌宠的语义（跳、拖、隐藏）在 [`drive_pet_clicks`]，且晚于 egui。
fn drive_input(
    screen: Res<Screen>,
    mascot: Res<Mascot>,
    settings: Res<Settings>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut petwin: ResMut<PetWin>,
    mut drag: ResMut<Drag>,
    mut hover: ResMut<Hover>,
    notify_state: Res<NotifyState>,
    mut q: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    if !screen.ready {
        return;
    }
    let Ok((window, mut cursor)) = q.single_mut() else {
        return;
    };
    // Use the window's *own* scale factor for all conversions (avoids
    // monitor-vs-window scale drift).
    let scale = window.resolution.scale_factor();
    let global = global_cursor_physical(scale);

    // Pointer position within the window in logical pixels (top-left origin).
    // Prefer Bevy's reported position (exact, same source the engine uses); fall back
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

    let win_w = if mascot.use_spine {
        crate::spine_pet::SPINE_WIN_W
    } else {
        WIN_W
    };
    let win_h = if mascot.use_spine {
        crate::spine_pet::SPINE_WIN_H
    } else {
        WIN_H
    };
    let center = if mascot.use_spine {
        crate::spine_pet::spine_pet_center()
    } else {
        pet_center()
    };
    let hit_r = if mascot.use_spine {
        crate::spine_pet::spine_hit_r()
    } else {
        HIT_R
    };

    let dist = local.distance(center);
    // Spine：HUD 在 x>=SPINE_PET_W；身体命中仅限左侧宠物视口，避免与侧栏几何重叠。
    let on_body = dist <= hit_r
        && (!mascot.use_spine || local.x < crate::spine_pet::SPINE_PET_W)
        && (!mascot.use_spine || local.y >= crate::spine_pet::spine_pet_viewport_top());
    let in_hud = if mascot.use_spine {
        if settings.hud_open {
            crate::spine_pet::spine_point_in_hud_panel(local.x, local.y)
        } else {
            crate::spine_pet::spine_point_in_gear(local.x, local.y)
        }
    } else if settings.hud_open {
        local.x >= WIN_W - HUD_W && local.x <= WIN_W && local.y >= 0.0 && local.y <= WIN_H
    } else {
        local.x >= WIN_W - GEAR_W && local.x <= WIN_W && local.y >= 0.0 && local.y <= GEAR_H
    };
    let in_bubble = if mascot.use_spine {
        notify_state.showing()
            && !settings.hud_open
            && crate::spine_pet::spine_point_in_notify(local.x, local.y)
    } else {
        notify_state.showing()
            && local.x >= 0.0
            && local.x <= win_w
            && local.y >= 0.0
            && local.y <= BUBBLE_H
    };
    let inside = on_body || in_hud || in_bubble;
    hover.on_body = on_body;
    hover.in_hud = in_hud;
    hover.in_bubble = in_bubble;
    hover.inside = inside;
    let near_r = if mascot.use_spine {
        crate::spine_pet::SPINE_PET_W * 1.15
    } else {
        NEAR_R
    };
    hover.near = dist <= near_r || in_hud || in_bubble;

    // Intercept the mouse over the body (or mid-drag); everywhere else the
    // click falls through to whatever is behind the window.
    let want_hit = inside || drag.active;
    if cursor.hit_test != want_hit {
        cursor.hit_test = want_hit;
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
}

/// 萌宠鼠标语义：在 egui HUD 处理完后再执行，避免与动画侧栏/⚙ 争抢。
fn drive_pet_clicks(
    mascot: Res<Mascot>,
    hover: Res<Hover>,
    mut settings: ResMut<Settings>,
    tray_cfg: Res<LaunchTrayConfig>,
    egui_wants: Res<EguiWantsInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut petwin: ResMut<PetWin>,
    mut drag: ResMut<Drag>,
    mut notify_state: ResMut<NotifyState>,
    mut click_tracker: ResMut<PetClickTracker>,
    mut q: Query<(&mut Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut pets: Query<&mut Pet>,
) {
    let scale = q
        .single()
        .map(|(w, _)| w.resolution.scale_factor())
        .unwrap_or(1.0);
    let global = global_cursor_physical(scale);

    // Spine 收起：点 ⚙ 条带展开（与几何 in_hud 一致）。
    if mascot.use_spine
        && !settings.hud_open
        && hover.in_hud
        && mouse.just_pressed(MouseButton::Left)
    {
        settings.hud_open = true;
        return;
    }

    if hover.in_bubble && mouse.just_pressed(MouseButton::Left) {
        notify_state.dismiss();
        return;
    }

    // egui 已展开侧栏时，让 HUD 控件优先（动画列表 / 收起）。
    if settings.hud_open && egui_wants.wants_any_pointer_input() {
        if mouse.just_pressed(MouseButton::Left) && !hover.on_body {
            click_tracker.last_on_body = false;
        }
        return;
    }

    // 收起态 ⚙ 条带：上面已处理展开，此处不再触发萌宠跳/拖。
    if mascot.use_spine && !settings.hud_open && hover.in_hud {
        return;
    }

    if egui_wants.wants_any_pointer_input() {
        if mouse.just_pressed(MouseButton::Left) && !hover.on_body {
            click_tracker.last_on_body = false;
        }
        return;
    }

    if mouse.just_pressed(MouseButton::Left) && hover.on_body {
        let now = std::time::Instant::now();
        let double_click = click_tracker.last_on_body
            && click_tracker
                .last_at
                .is_some_and(|t| now.duration_since(t).as_millis() <= PET_DOUBLE_CLICK_MS);
        click_tracker.last_at = Some(now);
        click_tracker.last_on_body = true;

        if double_click && matches!(tray_cfg.mode, TrayMode::StarlinkDev { .. }) {
            if let Ok((mut window, _)) = q.single_mut() {
                window.visible = false;
            }
            drag.active = false;
            click_tracker.last_at = None;
            return;
        }

        if let Some(g) = global {
            drag.active = true;
            drag.offset = g - petwin.pos;
        }
        if let Ok(mut pet) = pets.single_mut() {
            pet.vy = JUMP_V;
        }
    } else if mouse.just_pressed(MouseButton::Left) && !hover.on_body {
        click_tracker.last_on_body = false;
    }

    if (mouse.just_pressed(MouseButton::Right) && hover.on_body) || keys.just_pressed(KeyCode::Escape)
    {
        match &tray_cfg.mode {
            TrayMode::StarlinkDev { .. } => {
                if let Ok((mut window, _)) = q.single_mut() {
                    window.visible = false;
                }
            }
            TrayMode::Deskpet => shutdown_now(),
        }
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

/// 打开 starlink-dev admin 页面（端口从 serve 写入的文件读取）。
fn open_starlink_admin(port_file: &std::path::Path) {
    let port = match std::fs::read_to_string(port_file) {
        Ok(raw) => raw.trim().to_string(),
        Err(e) => {
            warn!(
                "deskpet: 无法读取 admin 端口文件 {}: {e}，回退 8901",
                port_file.display()
            );
            "8901".into()
        }
    };
    if port.is_empty() {
        warn!("deskpet: admin 端口文件为空，回退 8901");
    }
    let port = if port.is_empty() { "8901" } else { &port };
    let url = format!("http://127.0.0.1:{port}/");
    open_path_or_url(&url);
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

/// Create the system-tray / macOS menu-bar icon.
fn create_tray(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    assets_dir: Res<DeskpetAssetsDir>,
    tray_cfg: Res<LaunchTrayConfig>,
) {
    let (tray_filename, tooltip, menu, show_menu_on_left_click) = match &tray_cfg.mode {
        TrayMode::StarlinkDev { .. } => (
            "tray-starlink-dev.png",
            "Starlink-dev".into(),
            Menu::new(vec![
                MenuItem::icon(
                    "admin",
                    "Starlink-dev running",
                    true,
                    MenuIconKind::GreenStatusDot,
                ),
                MenuItem::separator(),
                MenuItem::check(
                    "show_pet",
                    "显示桌面宠",
                    true,
                    tray_cfg.start_visible,
                ),
                MenuItem::separator(),
                MenuItem::common("quit", "Quit Starlink-dev", true, None),
            ]),
            true,
        ),
        TrayMode::Deskpet => (
            "tray.png",
            "deskpet — click to show/hide".into(),
            Menu::new(vec![
                MenuItem::common("show", "Show", true, None),
                MenuItem::common("hide", "Hide", true, None),
                MenuItem::separator(),
                MenuItem::common("docs", "Reminder Protocol Docs", true, None),
                MenuItem::separator(),
                MenuItem::common("quit", "Quit", true, None),
            ]),
            false,
        ),
    };

    let tray_path = assets_dir.0.join(tray_filename);
    let handle = match crate::tray_icon::preload_tray_png(&tray_path, &mut images) {
        Ok(h) => {
            let hint = if show_menu_on_left_click {
                "左键打开菜单"
            } else {
                "左键显隐萌宠，右键菜单"
            };
            log::info!(
                "deskpet: 菜单栏图标已加载 {}（macOS 若在 « 溢出区请展开；{hint}）",
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
        tooltip: Some(tooltip),
        menu,
        show_menu_on_left_click,
    });
}

/// Tray menu actions.
fn tray_menu(
    mut reader: MessageReader<MenuMessage>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut tray: ResMut<TrayIcon>,
    tray_cfg: Res<LaunchTrayConfig>,
) {
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    for e in reader.read() {
        match e.id.0.as_str() {
            "show" => window.visible = true,
            "hide" => window.visible = false,
            "docs" => open_protocol_docs(),
            "admin" => {
                if let TrayMode::StarlinkDev { admin_port_file } = &tray_cfg.mode {
                    open_starlink_admin(admin_port_file);
                }
            }
            "show_pet" => {
                // MenuMessage 触发时 bevy 侧 checkbox 尚未更新，取反即为用户意图。
                let show = !tray.menu.is_checked("show_pet").unwrap_or(false);
                window.visible = show;
                let _ = tray.menu.set_checked("show_pet", show);
            }
            "quit" => shutdown_now(),
            _ => {}
        }
    }
}

/// 萌宠显隐与菜单 checkbox 保持一致（notify 弹出、双击隐藏等）。
fn sync_starlink_tray_pet_check(
    tray_cfg: Res<LaunchTrayConfig>,
    mut tray: ResMut<TrayIcon>,
    window: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
) {
    if !matches!(tray_cfg.mode, TrayMode::StarlinkDev { .. }) {
        return;
    }
    let Ok(w) = window.single() else {
        return;
    };
    if tray.menu.is_checked("show_pet") != Some(w.visible) {
        let _ = tray.menu.set_checked("show_pet", w.visible);
    }
}

/// Left-click toggles the mascot's visibility; a Windows double-click hides it.
/// `bevy_tray_icon` only bridges *menu* events into Bevy, so we poll the
/// tray-icon click channel directly here.
fn tray_clicks(
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    tray_cfg: Res<LaunchTrayConfig>,
) {
    if matches!(tray_cfg.mode, TrayMode::StarlinkDev { .. }) {
        return;
    }
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
/// terminal is not active, so it never receives cursor/click events and the
/// mascot can't be grabbed. When the pointer approaches the window we focus it
/// (which activates the app on macOS), so the events start flowing.
/// `NonSendMarker` pins this system to the main thread, which is required to
/// touch the `WINIT_WINDOWS` thread-local.
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
    // already flowing by the time the pointer reaches the body.
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
    mascot: Res<Mascot>,
    settings: Res<Settings>,
    mut petwin: ResMut<PetWin>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !screen.ready {
        return;
    }
    let Ok(mut window) = window.single_mut() else {
        return;
    };
    let (win_w, win_h) = if mascot.use_spine {
        (crate::spine_pet::SPINE_WIN_W, crate::spine_pet::SPINE_WIN_H)
    } else {
        (WIN_W, WIN_H)
    };
    let win_w_px = win_w * screen.scale;
    let win_h_px = win_h * screen.scale;
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
    mut pets: Query<(&mut Pet, &mut Transform), (Without<Eye>, Without<crate::spine_pet::SpinePet>)>,
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

/// Turn an RPC `pet/control` Hop request into an actual jump impulse.
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

fn notify_bubble(
    mut contexts: EguiContexts,
    mascot: Res<Mascot>,
    settings: Res<Settings>,
    state: Res<NotifyState>,
) {
    let Some(notice) = state.current.as_ref() else {
        return;
    };
    if mascot.use_spine && settings.hud_open {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let [r, g, b] = notice.level.accent();
    let accent = egui::Color32::from_rgb(r, g, b);
    let glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 235);
    let body_col = egui::Color32::from_rgb(228, 231, 238);
    let (bubble_pos, bubble_w, bubble_h, title_size, body_size, max_body_chars) = if mascot.use_spine {
        let n = crate::spine_pet::spine_notify_rect();
        (egui::pos2(n.x, n.y), n.w, n.h, 10.0_f32, 9.0_f32, 52_usize)
    } else {
        (egui::pos2(6.0, 6.0), WIN_W - 12.0, WIN_H * 0.5, 12.0_f32, 11.0_f32, 120_usize)
    };

    egui::Area::new(egui::Id::new("notify_bubble"))
        .order(egui::Order::Background)
        .fixed_pos(bubble_pos)
        .show(ctx, |ui| {
            ui.set_max_width(bubble_w);
            if mascot.use_spine {
                ui.set_max_height(bubble_h);
            }
            egui::Frame::new()
                .fill(glass)
                .stroke(egui::Stroke::new(1.0, accent))
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .show(ui, |ui| {
                    ui.set_max_width(bubble_w - 10.0);
                    if let Some(title) = &notice.title {
                        ui.label(
                            egui::RichText::new(title)
                                .strong()
                                .color(accent)
                                .size(title_size),
                        );
                    }
                    let body = truncate_notify_body(&notice.body, max_body_chars);
                    ui.label(egui::RichText::new(body).color(body_col).size(body_size));
                });
        });
}

fn truncate_notify_body(body: &str, max_chars: usize) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>()
        + "…"
}

/// egui 默认字体不含 CJK，侧边栏中文会显示为「□」乱码；加载系统字体。
fn install_egui_cjk_font(ctx: &egui::Context) {
    let Some(bytes) = load_system_cjk_font() else {
        log::warn!("deskpet: 未找到系统中文字体，HUD 中文可能显示为方框");
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    let mut font_data = egui::FontData::from_owned(bytes);
    font_data.index = 0;
    fonts.font_data.insert("cjk".to_owned(), font_data.into());
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());
    ctx.set_fonts(fonts);
}

fn load_system_cjk_font() -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        for path in [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                log::info!("deskpet: HUD 中文字体 → {path}");
                return Some(bytes);
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for path in [
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for path in [
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
    }
    None
}

fn hud_system(
    mut contexts: EguiContexts,
    mascot: Res<Mascot>,
    mut settings: ResMut<Settings>,
    mut spine_hud: Option<ResMut<crate::spine_pet::SpineHud>>,
    mut styled: Local<bool>,
) {
    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    if !*styled {
        *styled = true;
        install_egui_cjk_font(ctx);
        let mut style = (*ctx.style()).clone();
        let scale = if mascot.use_spine {
            crate::spine_pet::SPINE_UI_SCALE
        } else {
            UI_SCALE
        };
        for (_text_style, font_id) in style.text_styles.iter_mut() {
            font_id.size *= scale;
        }
        style.spacing.button_padding = egui::vec2(3.0, 2.0);
        style.spacing.interact_size.y *= scale;
        style.spacing.item_spacing = egui::vec2(3.0, 2.0);
        ctx.set_style(style);
    }

    if mascot.use_spine {
        crate::spine_pet::spine_hud_panel(ctx, &mut settings, spine_hud.as_deref_mut());
        return;
    }

    let glass = egui::Color32::from_rgba_unmultiplied(18, 20, 28, 205);

    if !settings.hud_open {
        let gear_pos = egui::pos2(WIN_W - GEAR_W + 4.0, 4.0);
        egui::Area::new(egui::Id::new("hud_gear"))
            .fixed_pos(gear_pos)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(glass)
                    .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(72, 78, 96)))
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin(2)
                    .show(ui, |ui| {
                        if ui
                            .button(egui::RichText::new("⚙").size(11.0))
                            .on_hover_text("动画面板")
                            .clicked()
                        {
                            settings.hud_open = true;
                        }
                    });
            });
        return;
    }

    let style = ctx.style();
    let frame = egui::Frame::side_top_panel(&style).fill(glass);
    let hud_w = HUD_W;
    egui::SidePanel::right("hud")
        .exact_width(hud_w)
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
            ui.checkbox(
                &mut settings.toon_shading,
                "柔和提亮（实验）",
            );
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
