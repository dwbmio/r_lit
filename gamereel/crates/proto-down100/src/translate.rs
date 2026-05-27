//! Snapshot 流 → gamereel-core MetaSceneList.
//!
//! 翻译策略 (跟 tools/down100-replay-render/compose_scene.py 完全一致):
//!
//! - **camera 跟玩家**: cam_x = 楼层中心 (稳定), cam_y = 玩家平均 y + lookahead.
//! - **clamp cam_y** 在 [FLOOR_Y_MIN-8, FLOOR_Y_MAX+8] — 玩家跌出 floor 窗口时
//!   camera 停在最低 floor 下方, 让"跌出画面"的视觉真实呈现.
//! - **楼层 + 玩家都是动态 node** — 每帧重算位置, timeline 用 move_to 连接.
//! - **自动 truncate** — 玩家最后一次有移动的帧 +30 帧尾巴, 避免 30 秒静止画面.
//!
//! 性能: 单局 typical ~500 snapshots × (2 players + 12 floors) = ~7000 actions,
//! 翻译耗时 < 5ms (纯内存运算).

use std::collections::HashMap;

use gamereel_core::stage::model::{
    meta_action::MetaAction,
    meta_node::{MetaNode, NodeAttr},
    meta_scene::{MetaScene, MetaSceneList},
};

use crate::decode::DecodedPayload;

// ─── 渲染参数 (跟 compose_scene.py 同步) ───────────────────
pub const CANVAS_W: u32 = 720;
pub const CANVAS_H: u32 = 1080;
pub const FPS: u32 = 30;

const SCALE_X: f32 = 28.0;
const SCALE_Y: f32 = 14.0;
const CAMERA_LOOKAHEAD_Y: f32 = -3.0;
const PLAYER_SPRITE_HALF: f32 = 16.0;
const FLOOR_SPRITE_HALF_W: f32 = 120.0;
const FLOOR_SPRITE_HALF_H: f32 = 8.0;

const MAX_FLOORS: usize = 200; // down100 最多 100 层 + 安全余量

const ID_BG: u64 = 1;
const ID_FLOOR_BASE: u64 = 100;
const ID_PLAYER_BASE: u64 = 1000;

const BG_TP_ID: &str = "0";
const FLOOR_TP_ID: &str = "1";
fn player_tp_id(i: usize) -> String { format!("{}", 2 + i) }

/// 玩家颜色调色板 — 跟 assets.rs 的 PLAYER_COLORS 长度对齐.
pub const PLAYER_COLORS: usize = 4;

pub struct Translation {
    pub scene_list: MetaSceneList,
    /// 总时长 (秒) — caller 给 RuntimeCtx::new(_, _, secs, fps) 用.
    pub duration_secs: f32,
    /// 真实出现的玩家数 (用于 assets 生成 player_0..player_N 的 PNG)
    pub player_count: usize,
    /// 玩家 ID 列表 (按出现顺序, 跟 player_<i> 索引对齐)
    pub player_ids: Vec<String>,
    /// 房间 ID (caller 用于命名输出 mp4)
    pub room_id: String,
}

fn world_to_canvas(wx: f32, wy: f32, cam_x: f32, cam_y: f32) -> (f32, f32) {
    let cx = (CANVAS_W as f32 / 2.0) + (wx - cam_x) * SCALE_X;
    let cy = (CANVAS_H as f32 / 2.0) - (wy - cam_y) * SCALE_Y;
    (cx, cy)
}

pub fn translate(decoded: &DecodedPayload) -> Translation {
    let snapshots = &decoded.snapshots;
    let room_id = decoded.header.room_id.clone();

    if snapshots.is_empty() {
        return Translation {
            scene_list: MetaSceneList { meta_scene_list: vec![] },
            duration_secs: 0.0,
            player_count: 0,
            player_ids: vec![],
            room_id,
        };
    }

    let t0_ms = snapshots[0].1;

    // ─── 收集 player_ids (按出现顺序) ───
    let mut player_ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (snap, _) in snapshots {
        for p in &snap.players {
            if seen.insert(p.player_id.clone()) {
                player_ids.push(p.player_id.clone());
            }
        }
    }

    // ─── 收集所有 floor (floor_idx → (y, left, right)) ───
    let mut floors_seen: HashMap<u32, (f32, f32, f32)> = HashMap::new();
    for (snap, _) in snapshots {
        for f in &snap.floors {
            floors_seen.entry(f.floor).or_insert((f.y, f.left, f.right));
        }
    }
    let mut sorted_floors: Vec<(u32, (f32, f32, f32))> = floors_seen
        .iter()
        .map(|(k, v)| (*k, *v))
        .collect();
    sorted_floors.sort_by_key(|(k, _)| *k);
    sorted_floors.truncate(MAX_FLOORS);

    // ─── camera clamp 范围 ───
    let floor_y_min = sorted_floors
        .iter()
        .map(|(_, (y, _, _))| *y)
        .fold(f32::INFINITY, f32::min);
    let floor_y_max = sorted_floors
        .iter()
        .map(|(_, (y, _, _))| *y)
        .fold(f32::NEG_INFINITY, f32::max);
    let floor_x_center = if !sorted_floors.is_empty() {
        sorted_floors
            .iter()
            .map(|(_, (_, l, r))| (l + r) / 2.0)
            .sum::<f32>()
            / sorted_floors.len() as f32
    } else {
        0.0
    };

    let get_camera = |snap: &crate::proto::down100::Down100Snapshot| -> (f32, f32) {
        if snap.players.is_empty() {
            return (floor_x_center, floor_y_min);
        }
        let avg_y: f32 = snap.players.iter().map(|p| p.y).sum::<f32>()
            / snap.players.len() as f32;
        let target_y = avg_y + CAMERA_LOOKAHEAD_Y;
        let clamped_y = target_y.max(floor_y_min - 8.0).min(floor_y_max + 8.0);
        (floor_x_center, clamped_y)
    };

    // ─── auto-truncate: 找最后一帧玩家状态变化 + 30 帧尾巴 ───
    let mut last_motion_idx = 0usize;
    let mut last_state: Option<Vec<(String, i32, i32)>> = None;
    for (i, (snap, _)) in snapshots.iter().enumerate() {
        let state: Vec<(String, i32, i32)> = snap.players.iter()
            .map(|p| (p.player_id.clone(), (p.x * 100.0) as i32, (p.y * 100.0) as i32))
            .collect();
        if last_state.as_ref() != Some(&state) {
            last_motion_idx = i;
            last_state = Some(state);
        }
    }
    let keep = (last_motion_idx + 30).min(snapshots.len());
    let snapshots = &snapshots[..keep];

    // ─── 节点 + timeline ───
    let mut nodes: Vec<MetaNode> = Vec::new();
    let mut timeline: HashMap<String, Vec<MetaAction>> = HashMap::new();

    // 背景 — 静态
    nodes.push(MetaNode::new_with_attr(
        ID_BG,
        BG_TP_ID,
        "bg",
        NodeAttr {
            pos: [0.0, 0.0, 0.0],
            active: true,
            is_static: true,
            ..Default::default()
        },
    ));

    // 楼层节点 — 每个 floor 一个 node, 初始位置用第一帧 cam
    let (cam0_x, cam0_y) = get_camera(&snapshots[0].0);
    let mut floor_node_ids: HashMap<u32, u64> = HashMap::new();
    for (offset, (floor_idx, (wy, left, right))) in sorted_floors.iter().enumerate() {
        let nid = ID_FLOOR_BASE + offset as u64;
        floor_node_ids.insert(*floor_idx, nid);
        let wx_center = (left + right) / 2.0;
        let (cx, cy) = world_to_canvas(wx_center, *wy, cam0_x, cam0_y);
        nodes.push(MetaNode::new_with_attr(
            nid,
            FLOOR_TP_ID,
            &format!("floor_{floor_idx}"),
            NodeAttr {
                pos: [cx - FLOOR_SPRITE_HALF_W, cy - FLOOR_SPRITE_HALF_H, 0.0],
                active: true,
                ..Default::default()
            },
        ));
    }

    // 玩家节点 — 每个 player 一个 node
    for (i, pid) in player_ids.iter().enumerate() {
        let nid = ID_PLAYER_BASE + i as u64;
        // 初始位置
        let mut first_pos = None;
        for (snap, _) in snapshots.iter() {
            if let Some(p) = snap.players.iter().find(|p| &p.player_id == pid) {
                let (cx, cy) = world_to_canvas(p.x, p.y, cam0_x, cam0_y);
                first_pos = Some([cx - PLAYER_SPRITE_HALF, cy - PLAYER_SPRITE_HALF, 0.0]);
                break;
            }
        }
        let pos = match first_pos {
            Some(p) => p,
            None => continue,
        };
        let tp_id = player_tp_id(i);
        nodes.push(MetaNode::new_with_attr(
            nid,
            &tp_id,
            &format!("player_{}", &pid[..pid.len().min(6)]),
            NodeAttr { pos, active: true, ..Default::default() },
        ));
    }

    // ─── 生成 timeline (每帧 → move_to actions) ───
    // prev_pos: node_id → (last_t_sec, last_pos)
    let mut prev_pos: HashMap<u64, (f32, [f32; 3])> = HashMap::new();
    // floors_seen 镜像副本, 跨帧更新 (本帧 snapshot.floors 出现的新位置覆盖旧值)
    let mut floors_live = floors_seen.clone();

    for (snap, ts_ms) in snapshots {
        let t_sec = (ts_ms - t0_ms) as f32 / 1000.0;
        let (cam_x, cam_y) = get_camera(snap);

        // 玩家
        for (i, pid) in player_ids.iter().enumerate() {
            let nid = ID_PLAYER_BASE + i as u64;
            let Some(p) = snap.players.iter().find(|p| &p.player_id == pid) else {
                continue;
            };
            let (cx, cy) = world_to_canvas(p.x, p.y, cam_x, cam_y);
            let pos = [cx - PLAYER_SPRITE_HALF, cy - PLAYER_SPRITE_HALF, 0.0];
            if let Some((prev_t, prev_p)) = prev_pos.get(&nid).cloned() {
                let duration = (t_sec - prev_t).max(0.001);
                timeline
                    .entry(nid.to_string())
                    .or_default()
                    .push(MetaAction::new_move_to(nid, prev_p, pos, prev_t, duration));
            }
            prev_pos.insert(nid, (t_sec, pos));
        }

        // 更新 floor live cache
        for f in &snap.floors {
            floors_live.insert(f.floor, (f.y, f.left, f.right));
        }

        // 全程已知 floor 重算位置 (即使本帧 snapshot 不含, 也要跟 camera 同步)
        for (floor_idx, nid) in &floor_node_ids {
            let (wy, left, right) = floors_live.get(floor_idx).copied().unwrap_or((0.0, -2.5, 2.5));
            let wx_center = (left + right) / 2.0;
            let (cx, cy) = world_to_canvas(wx_center, wy, cam_x, cam_y);
            let pos = [cx - FLOOR_SPRITE_HALF_W, cy - FLOOR_SPRITE_HALF_H, 0.0];
            // 屏幕外 floor 不写 timeline action 节省
            let off_screen = cy < -200.0 || cy > CANVAS_H as f32 + 200.0;
            if !off_screen {
                if let Some((prev_t, prev_p)) = prev_pos.get(nid).cloned() {
                    let duration = (t_sec - prev_t).max(0.001);
                    timeline
                        .entry(nid.to_string())
                        .or_default()
                        .push(MetaAction::new_move_to(*nid, prev_p, pos, prev_t, duration));
                }
            }
            prev_pos.insert(*nid, (t_sec, pos));
        }
    }

    let duration_secs = (snapshots.last().unwrap().1 - t0_ms) as f32 / 1000.0 + 0.5;

    let scene = MetaScene {
        name: format!("down100_{room_id}"),
        clear_tp_id: Some(BG_TP_ID.to_string()),
        textures: build_textures(&player_ids),
        nodes,
        timeline,
    };
    let scene_list = MetaSceneList {
        meta_scene_list: vec![scene],
    };

    Translation {
        scene_list,
        duration_secs,
        player_count: player_ids.len(),
        player_ids,
        room_id,
    }
}

/// 构造 textures 路径列表 — caller 把 assets 写到磁盘后, RuntimeCtx.set_source_path
/// 指向该目录即可.
fn build_textures(player_ids: &[String]) -> Vec<String> {
    let mut out = vec!["bg.png".to_string(), "floor.png".to_string()];
    for i in 0..player_ids.len() {
        out.push(format!("player_{}.png", i % PLAYER_COLORS));
    }
    out
}
