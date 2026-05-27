//! base64 + zstd decode + varint length-prefixed protobuf stream parsing.
//!
//! 输入: `envelope.kwargs.payload` 的 JSON value (含 zstd_blob base64 字符串).
//! 输出: [`DecodedPayload`] (header + 按 kind 分类的事件列表).

use prost::Message;
use thiserror::Error;

use crate::proto::{down100, replay, titan};

#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    #[error("zstd decode error: {0}")]
    Zstd(#[from] std::io::Error),
    #[error("protobuf decode error: {0}")]
    Prost(#[from] prost::DecodeError),
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("invalid varint at offset {0}")]
    Varint(usize),
}

/// 解出来的 down100 局数据 — 已按 kind 分桶, 渲染端按需取用.
#[derive(Debug, Default)]
pub struct DecodedPayload {
    pub header: replay::ReplayHeader,
    /// 服务端权威 snapshot 流 (按 ts_ms 顺序) — 渲染主线
    pub snapshots: Vec<(down100::Down100Snapshot, i64)>,
    /// 房间事件 (RoomStarted / PlatformHit / Rescue / GameOver / ...) — HUD / 特效
    pub events: Vec<(down100::Down100Event, i64)>,
    /// 远端玩家输入回放 (10Hz)
    pub remote_inputs: Vec<(down100::Down100RemoteInput, i64)>,
    /// 原始玩家输入 (备用对账, 一般不渲染)
    pub raw_inputs: Vec<(down100::Down100Input, i64)>,
    /// 玩家档案 (昵称 / 头像 / 等级)
    pub rosters: Vec<(titan::PlayerRoster, i64)>,
}

/// 解码主入口. `payload_value` 是 envelope.kwargs.payload (JSON object).
pub fn decode_payload(payload: &serde_json::Value) -> Result<DecodedPayload, DecodeError> {
    let zstd_b64 = payload
        .get("zstd_blob")
        .and_then(|v| v.as_str())
        .ok_or(DecodeError::MissingField("zstd_blob"))?;

    let compressed = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        zstd_b64,
    )?;
    let raw = zstd::decode_all(&compressed[..])?;

    let mut out = DecodedPayload::default();
    let mut off = 0usize;

    // header
    let (n, new_off) = read_varint(&raw, off)?;
    off = new_off;
    out.header = replay::ReplayHeader::decode(&raw[off..off + n as usize])?;
    off += n as usize;

    // events
    while off < raw.len() {
        let (n, new_off) = read_varint(&raw, off)?;
        off = new_off;
        let body = &raw[off..off + n as usize];
        off += n as usize;
        let ev = replay::ReplayEvent::decode(body)?;

        // ReplayEventKind 是数字, 不强类型, 按 i32 比较
        use replay::ReplayEventKind as K;
        let kind = K::try_from(ev.kind).unwrap_or(K::ReplayUnspecified);
        let ts = ev.ts_ms;
        let payload = &ev.payload[..];
        match kind {
            K::ReplayDown100Snapshot => {
                if let Ok(s) = down100::Down100Snapshot::decode(payload) {
                    out.snapshots.push((s, ts));
                }
            }
            K::ReplayDown100Event => {
                if let Ok(e) = down100::Down100Event::decode(payload) {
                    out.events.push((e, ts));
                }
            }
            K::ReplayDown100RemoteInput => {
                if let Ok(r) = down100::Down100RemoteInput::decode(payload) {
                    out.remote_inputs.push((r, ts));
                }
            }
            K::ReplayDown100InputRaw => {
                if let Ok(i) = down100::Down100Input::decode(payload) {
                    out.raw_inputs.push((i, ts));
                }
            }
            K::ReplayDown100Roster => {
                if let Ok(r) = titan::PlayerRoster::decode(payload) {
                    out.rosters.push((r, ts));
                }
            }
            // v1.2.2 老 FPS kind 这里也允许出现 (但 down100 不会发), 忽略.
            _ => {}
        }
    }

    Ok(out)
}

/// proto3 varint 解码. 返回 (value, new_offset).
fn read_varint(buf: &[u8], mut off: usize) -> Result<(u64, usize), DecodeError> {
    let start = off;
    let mut v: u64 = 0;
    let mut shift: u32 = 0;
    loop {
        if off >= buf.len() {
            return Err(DecodeError::Varint(start));
        }
        let b = buf[off];
        off += 1;
        v |= ((b & 0x7F) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok((v, off));
        }
        shift += 7;
        if shift > 63 {
            return Err(DecodeError::Varint(start));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_basic() {
        // 0x05 → 5
        assert_eq!(read_varint(&[0x05], 0).unwrap(), (5, 1));
        // 0x80 0x01 → 128
        assert_eq!(read_varint(&[0x80, 0x01], 0).unwrap(), (128, 2));
        // 0xAC 0x02 → 300
        assert_eq!(read_varint(&[0xAC, 0x02], 0).unwrap(), (300, 2));
    }
}
