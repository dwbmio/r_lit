//! `proto-down100` — titan-forge Phase 7.8 Replay 协议解码 + gamereel scene 翻译.
//!
//! ## 输入
//!
//! 一段 base64 编码的 `ReplayShipPayload.zstd_blob`, 从 rustyme `titan:write` 队列
//! 拿到 (envelope.kwargs.payload.zstd_blob). 解开 zstd 后是:
//!
//! ```text
//! [varint len][ReplayHeader (proto)] [varint len][ReplayEvent (proto)] *
//! ```
//!
//! 每条 ReplayEvent 含 `(tick, ts_ms, player_id, kind, payload)`, 其中 `payload`
//! 是按 `kind` 映射的具体 proto Message bytes (见 `kind` 表).
//!
//! ## 输出
//!
//! [`Translation`] — gamereel-core 的 `MetaSceneList` + 渲染所需 PNG 资源 bytes
//! (背景/楼层/玩家). caller 把资源写到磁盘 + 构造 `StageMgr`, 调 `start_gen_first`
//! 输出 mp4.
//!
//! ## kind ↔ Message ↔ msg_id 映射 (与 titan-forge v1.5.0 一致)
//!
//! | ReplayEventKind                  | msg_id | proto Message                       |
//! |----------------------------------|--------|-------------------------------------|
//! | REPLAY_DOWN100_SNAPSHOT     = 10 | 306    | `titan.down100.Down100Snapshot`     |
//! | REPLAY_DOWN100_EVENT        = 11 | 307    | `titan.down100.Down100Event`        |
//! | REPLAY_DOWN100_REMOTE_INPUT = 12 | 309    | `titan.down100.Down100RemoteInput`  |
//! | REPLAY_DOWN100_INPUT_RAW    = 13 | 301    | `titan.down100.Down100Input`        |
//! | REPLAY_DOWN100_ROSTER       = 14 | 90     | `titan.PlayerRoster`                |
//!
//! ## 设计
//!
//! - **不重跑游戏逻辑** — 服务端 snapshot 流就是渲染所需的全部信息.
//! - **camera 跟随玩家 + clamp** — 跟 titan-forge tools/compose_scene.py 一致策略.
//! - **资源 PNG 内存生成** — 不依赖外部图集, 用 `image` + `imageproc` 一次性画好.
//! - **不实现 ProtocolParser trait** — gamereel-cli 的 ParsedReplay 只能返回 metadata,
//!   渲染场景翻译目前不通过它. 我们直接暴露 `decode + translate` 给 binary caller.

pub mod assets;
pub mod decode;
pub mod translate;

/// prost-build 自动生成的 protobuf 模块.
pub mod proto {
    /// titan.down100.* (Down100Snapshot / Down100Event / Down100Input / ...)
    pub mod down100 {
        include!(concat!(env!("OUT_DIR"), "/titan.down100.rs"));
    }
    /// titan.replay.* (ReplayHeader / ReplayEvent / ReplayEventKind / ReplayShipPayload)
    pub mod replay {
        include!(concat!(env!("OUT_DIR"), "/titan.replay.rs"));
    }
    /// titan.* (PlayerRoster / PlayerProfile / 既有所有公共消息)
    pub mod titan {
        include!(concat!(env!("OUT_DIR"), "/titan.rs"));
    }
}

pub use decode::{decode_payload, DecodedPayload, DecodeError};
pub use translate::{translate, Translation};
