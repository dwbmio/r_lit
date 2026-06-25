//! `pet/*` methods. Read and mutate the running mascot's state via Bevy
//! resources. Cross-cutting concerns (window visibility, world state) are
//! also exposed so external tools can drive the pet without UI access.

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, Window};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::rpc::{AppError, Method, RpcError};
use crate::{Mascot, PetWin, Screen, Settings, Walk};
use serde_json::Value;

// ---- pet/state -------------------------------------------------------------

#[derive(Debug, Serialize, ToSchema)]
pub struct StateResult {
    /// Window top-left in physical pixels: [x, y].
    pub position: [f32; 2],
    /// Current `walk_speed` setting (px / s).
    pub walk_speed: f32,
    /// Current walk target (px). `walk_speed` is meaningless if not moving.
    pub walk_target: f32,
    /// Whether the idle wander system is currently moving.
    pub moving: bool,
    /// Whether the procedural slime is loaded (false = GLB mascot).
    pub is_slime: bool,
    /// Currently loaded GLB filename, or empty when slime.
    pub mascot_glb: String,
    /// Whether the pet window is currently visible.
    pub visible: bool,
}

pub struct StateMethod;

impl Method for StateMethod {
    fn name(&self) -> &'static str {
        "pet/state"
    }
    fn description(&self) -> &'static str {
        "Snapshot of the pet's current state: position, walk params, mascot \
         kind, window visibility. Read-only."
    }
    fn invoke(&self, world: &mut World, _params: Value) -> Result<Value, RpcError> {
        let pos = world
            .get_resource::<PetWin>()
            .map(|w| [w.pos.x, w.pos.y])
            .unwrap_or([0.0, 0.0]);
        let walk_speed = world
            .get_resource::<Settings>()
            .map(|s| s.walk_speed)
            .unwrap_or(0.0);
        let walk_target = world.get_resource::<Walk>().map(|w| w.target_x).unwrap_or(0.0);
        let moving = world.get_resource::<Walk>().map(|w| w.moving).unwrap_or(false);
        let (is_slime, mascot_glb) = world
            .get_resource::<Mascot>()
            .map(|m| (!m.use_glb, m.glb.clone()))
            .unwrap_or((true, String::new()));
        let visible = world
            .query_filtered::<&Window, With<PrimaryWindow>>()
            .single(world)
            .map(|w| w.visible)
            .unwrap_or(false);

        let result = StateResult {
            position: pos,
            walk_speed,
            walk_target,
            moving,
            is_slime,
            mascot_glb,
            visible,
        };
        Ok(serde_json::to_value(result).map_err(|e| RpcError::Internal(e.to_string()))?)
    }
}

#[utoipa::path(
    post,
    path = "/m/pet/state",
    tag = "pet",
    responses(
        (status = 200, description = "Snapshot", body = StateResult),
    ),
)]
#[allow(dead_code)]
pub fn state_path() -> StateResult {
    unimplemented!("openapi stub")
}

// ---- pet/control -----------------------------------------------------------

#[derive(Debug, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PetAction {
    /// Trigger a vertical hop (sets `hop_request`).
    Hop,
    /// Show the pet window.
    Show,
    /// Hide the pet window.
    Hide,
    /// Update `walk_speed`. Requires `value` in [0.0, 1000.0].
    SetSpeed,
    /// Instant teleport. Requires `x`, `y` in physical pixels.
    /// Halts any in-progress walk.
    WalkTo,
    /// Toggle idle wander on/off. Requires `enabled`.
    SetWander,
    /// Switch to a GLB mascot by filename (e.g. "block.glb", "blast.glb").
    /// Requires `name`. Triggers the same switch path the HUD "Switch" button uses.
    SetGlb,
    /// Toggle the HUD panel open/closed.
    ToggleHud,
    /// Reset the pet window to the center of the primary monitor.
    ResetPosition,
    /// Cleanly shut down the deskpet process. Replies first, then exits.
    Quit,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ControlParams {
    pub action: PetAction,
    /// Required for `set_speed` (f32, [0.0, 1000.0]).
    #[serde(default)]
    pub value: Option<f32>,
    /// Required for `walk_to`.
    #[serde(default)]
    pub x: Option<f32>,
    /// Required for `walk_to`.
    #[serde(default)]
    pub y: Option<f32>,
    /// Required for `set_wander`.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Required for `set_glb`.
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ControlResult {
    pub applied: bool,
    /// Human-readable note (e.g., "walk_speed=80.0").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub struct ControlMethod;

impl Method for ControlMethod {
    fn name(&self) -> &'static str {
        "pet/control"
    }
    fn description(&self) -> &'static str {
        "Dispatch an action to the pet: hop, show, hide, or set walk speed."
    }
    fn invoke(&self, world: &mut World, params: Value) -> Result<Value, RpcError> {
        let p: ControlParams = serde_json::from_value(params)
            .map_err(|e| RpcError::InvalidParams(e.to_string()))?;

        match p.action {
            PetAction::Hop => {
                world
                    .get_resource_mut::<Settings>()
                    .ok_or_else(|| RpcError::Internal("Settings resource missing".into()))?
                    .hop_request = true;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some("hop_request=true".into()),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::Show | PetAction::Hide => {
                let visible = matches!(p.action, PetAction::Show);
                if let Ok(mut window) = world
                    .query_filtered::<&mut Window, With<PrimaryWindow>>()
                    .single_mut(world)
                {
                    window.visible = visible;
                } else {
                    return Err(RpcError::Server {
                        code: AppError::ActionNotApplicable.code(),
                        message: AppError::ActionNotApplicable.message().into(),
                        source: None,
                        data: Some(serde_json::json!({ "action": "show/hide" })),
                    });
                }
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("visible={visible}")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::SetSpeed => {
                let v = p.value.ok_or_else(|| {
                    RpcError::InvalidParams("set_speed requires 'value'".into())
                })?;
                if !(0.0..=1000.0).contains(&v) {
                    return Err(RpcError::Server {
                        code: AppError::InvalidAction.code(),
                        message: AppError::InvalidAction.message().into(),
                        source: None,
                        data: Some(serde_json::json!({ "value": v, "allowed": [0.0, 1000.0] })),
                    });
                }
                let mut settings = world
                    .get_resource_mut::<Settings>()
                    .ok_or_else(|| RpcError::Internal("Settings resource missing".into()))?;
                settings.walk_speed = v;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("walk_speed={v}")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::WalkTo => {
                let x = p.x.ok_or_else(|| {
                    RpcError::InvalidParams("walk_to requires 'x'".into())
                })?;
                let y = p.y.ok_or_else(|| {
                    RpcError::InvalidParams("walk_to requires 'y'".into())
                })?;
                // Sanity-check bounds: allow negative (multi-monitor setups)
                // but reject absurd values (would mean caller is confused).
                if !(-100_000.0..=100_000.0).contains(&x)
                    || !(-100_000.0..=100_000.0).contains(&y)
                {
                    return Err(RpcError::Server {
                        code: AppError::InvalidAction.code(),
                        message: AppError::InvalidAction.message().into(),
                        source: None,
                        data: Some(serde_json::json!({
                            "x": x, "y": y,
                            "allowed": [-100_000.0, 100_000.0],
                        })),
                    });
                }
                // Set position + halt walk as two separate borrows — Rust's
                // borrow checker rejects two `get_resource_mut` in one scope.
                world
                    .get_resource_mut::<PetWin>()
                    .ok_or_else(|| RpcError::Internal("PetWin resource missing".into()))?
                    .pos = bevy::prelude::Vec2::new(x, y);
                world
                    .get_resource_mut::<Walk>()
                    .ok_or_else(|| RpcError::Internal("Walk resource missing".into()))?
                    .moving = false;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("pos=({x}, {y})")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::SetWander => {
                let enabled = p.enabled.ok_or_else(|| {
                    RpcError::InvalidParams("set_wander requires 'enabled'".into())
                })?;
                world
                    .get_resource_mut::<Settings>()
                    .ok_or_else(|| RpcError::Internal("Settings resource missing".into()))?
                    .wander = enabled;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("wander={enabled}")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::Quit => {
                // Send AppExit (Bevy 0.18 renamed events → messages).
                // Bevy processes it at the end of the current frame's
                // schedule — the reply goes out first (drain system is in
                // the same Update chain), so callers always get a
                // successful response before the process exits.
                world.write_message(AppExit::Success);
                info!("deskpet: AppExit requested via pet/control quit");
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some("AppExit sent — process will exit after this frame".into()),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::SetGlb => {
                let name = p
                    .name
                    .clone()
                    .ok_or_else(|| RpcError::InvalidParams("set_glb requires 'name'".into()))?;
                // Sanity check: only allow simple filenames (no path
                // separators — the GLB loader resolves under assets/).
                if name.contains('/') || name.contains('\\') || name.contains("..") {
                    return Err(RpcError::Server {
                        code: AppError::InvalidAction.code(),
                        message: AppError::InvalidAction.message().into(),
                        source: None,
                        data: Some(serde_json::json!({
                            "name": name,
                            "hint": "filename only, no path separators",
                        })),
                    });
                }
                // Update Mascot + trigger the same switch path the HUD uses.
                world
                    .get_resource_mut::<Mascot>()
                    .ok_or_else(|| RpcError::Internal("Mascot resource missing".into()))?
                    .glb = name.clone();
                world
                    .get_resource_mut::<Mascot>()
                    .ok_or_else(|| RpcError::Internal("Mascot resource missing".into()))?
                    .use_glb = true;
                world
                    .get_resource_mut::<Settings>()
                    .ok_or_else(|| RpcError::Internal("Settings resource missing".into()))?
                    .switch_request = true;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("glb={name}, switch_request=true")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::ToggleHud => {
                let mut settings = world
                    .get_resource_mut::<Settings>()
                    .ok_or_else(|| RpcError::Internal("Settings resource missing".into()))?;
                settings.hud_open = !settings.hud_open;
                let now = settings.hud_open;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("hud_open={now}")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
            PetAction::ResetPosition => {
                // Center on primary monitor using cached Screen geometry.
                // Window dims are the same constants main.rs uses to construct
                // the WindowResolution. Duplicated here so this method doesn't
                // need a back-reference into main.rs.
                const WIN_W: f32 = 310.0;
                const WIN_H: f32 = 200.0;
                let (origin, size) = world
                    .get_resource::<Screen>()
                    .map(|s| (s.origin, s.size))
                    .unwrap_or((bevy::prelude::Vec2::ZERO, bevy::prelude::Vec2::ZERO));
                let cx = origin.x + (size.x - WIN_W) / 2.0;
                let cy = origin.y + (size.y - WIN_H) / 2.0;
                world
                    .get_resource_mut::<PetWin>()
                    .ok_or_else(|| RpcError::Internal("PetWin resource missing".into()))?
                    .pos = bevy::prelude::Vec2::new(cx, cy);
                world
                    .get_resource_mut::<Walk>()
                    .ok_or_else(|| RpcError::Internal("Walk resource missing".into()))?
                    .moving = false;
                Ok(serde_json::to_value(ControlResult {
                    applied: true,
                    note: Some(format!("pos=({cx:.0}, {cy:.0})")),
                })
                .map_err(|e| RpcError::Internal(e.to_string()))?)
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/m/pet/control",
    tag = "pet",
    request_body = ControlParams,
    responses(
        (status = 200, description = "Action applied", body = ControlResult),
        (status = 400, description = "Invalid params"),
        (status = 409, description = "Action not applicable in current state"),
        (status = 422, description = "Invalid action (e.g., out-of-range value)"),
    ),
)]
#[allow(dead_code)]
pub fn control_path(_params: ControlParams) -> ControlResult {
    unimplemented!("openapi stub")
}