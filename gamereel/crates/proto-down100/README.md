# proto-down100

Protocol parser + scene translator for **"是男人就下100层"** (titan-forge `down100` 玩法) Replay fanout 协议.

## Pipeline

```
rustyme:titan:write queue        gamereel-core
       │                              │
       ▼                              ▼
envelope.kwargs.payload    →   MetaSceneList  →  StageMgr  →  mp4
  (zstd_blob base64)         (此 crate 输出)
```

## Source of truth

- **proto schema**: 跟 `titan-forge/proto/{replay,down100,messages,fps_events,moba_events}.proto` 同步 (v1.5.0+)
- **fanout 上游**: titan-forge Phase 7.8 — `crates/titan-rules-down100/src/room_actor.rs::maybe_spawn_replay_fanout`
- **rust 消费端**: 配套的 `down100-replay-renderer` binary (本仓库)

升级 proto 时:

```bash
cp /path/to/titan-forge/proto/{replay,down100,messages,fps_events,moba_events}.proto \
   crates/proto-down100/proto/
cargo check -p proto-down100
```

## API

```rust
use proto_down100::{decode_payload, translate, assets};

let envelope: serde_json::Value = serde_json::from_str(redis_value)?;
let payload = &envelope["kwargs"]["payload"];

// 1. base64 + zstd + protobuf decode
let decoded = decode_payload(payload)?;
println!("snapshots={} events={}", decoded.snapshots.len(), decoded.events.len());

// 2. translate 成 gamereel scene + duration 估算
let trans = translate(&decoded);

// 3. 写资源 PNG 到 caller 目录
let tmp = tempfile::TempDir::new()?;
assets::write_assets(tmp.path(), &trans.room_id, &trans.player_ids)?;

// 4. 调 gamereel-core StageMgr 渲染 mp4 (见 down100-replay-renderer bin)
```

## 翻译策略

- **camera**: cam_x = 楼层中心 (稳定), cam_y = 玩家平均 y + lookahead, clamp 在
  `[FLOOR_Y_MIN-8, FLOOR_Y_MAX+8]` (玩家跌出 floor 窗口时 camera 停在最低 floor 下方,
  视觉上表现"玩家跌出画面").
- **楼层 + 玩家** 都是动态 node, timeline 用 `move_to` 序列连接每帧位置.
- **auto-truncate**: 玩家最后一次有移动的帧 +30 帧尾巴, 避免长时间静止画面.
- **资源 PNG 内存生成** (image + imageproc + ab_glyph), 无外部图集依赖.

## kind ↔ Message ↔ msg_id 映射

| ReplayEventKind                  | msg_id | proto Message                       |
|----------------------------------|--------|-------------------------------------|
| REPLAY_DOWN100_SNAPSHOT     = 10 | 306    | `titan.down100.Down100Snapshot`     |
| REPLAY_DOWN100_EVENT        = 11 | 307    | `titan.down100.Down100Event`        |
| REPLAY_DOWN100_REMOTE_INPUT = 12 | 309    | `titan.down100.Down100RemoteInput`  |
| REPLAY_DOWN100_INPUT_RAW    = 13 | 301    | `titan.down100.Down100Input`        |
| REPLAY_DOWN100_ROSTER       = 14 | 90     | `titan.PlayerRoster`                |

## 为什么不实现 `ProtocolParser` trait?

gamereel-core 的 `ParsedReplay` 当前只能返回 `{filename, metadata, frames}` (M5 stub),
不能携带 `MetaSceneList`. 翻译产物必须直接喂给 `StageMgr`, 所以本 crate 暴露 `decode + translate`
给 binary caller 直接使用, 不走 inventory registry.

未来 gamereel-core 把 scene 加进 ParsedReplay 后, 可以补一层 `ProtocolParser` adapter 包一下.
