# audiogen — Pure-Rust game audio pipeline

Pure-Rust CLI for game audio assets. Used by `gd4_busy_bus` games (currently wired
into `classroom`). Replaces `scripts/racing_audiogen.py` style of one-off Python
per game — single tool, JSON-driven, multi-format codec trait.

## Features

- **Multi-format codec trait** (`Mp3Codec`, `WavCodec`, `OggCodec`) — one impl per
  format, easy to extend. Adding a format = one file.
- **MiniMax async pipeline** — `submit` (SSH-LPUSH to VPS rustyme redis queue)
  returns immediately; `collect` (BRPOP) when ready. Pending state tracked in
  `origin_res/<game>/audio/.pending.json`.
- **Procedural SFX** — 6 hand-rolled generators (woodblock / ding / pluck /
  thud / fanfare / noise). Deterministic, zero-cost, no network.
- **Pure Rust, no system coupling** — no ffmpeg / no native libs. `shine-rs`
  encodes MP3, hand-rolled WAV codec, `russh` does SSH.
- **v1 limits**: encode only (decode = TODO), no resampling / no trim / no
  normalize. AI results land as-is.

## Crate deps (8 total)

```
clap, serde, serde_json, anyhow, thiserror,
shine-rs (MP3 encode),
russh + russh-keys + tokio + futures (SSH),
base64
```

## Usage

```bash
# 1. List jobs
audiogen --jobs jobs/classroom.json status

# 2. Procedural SFX (immediate, no network)
audiogen --jobs jobs/classroom.json sfx
audiogen --jobs jobs/classroom.json sfx --only sfx_click,sfx_chain

# 3. Submit AI jobs to VPS (async — returns immediately)
audiogen --jobs jobs/classroom.json submit
AUDIOGEN_VPS=root@vps.example.com AUDIOGEN_SSH_KEY=~/.ssh/id_rsa \
    audiogen --jobs jobs/classroom.json submit --only bgm_lobby

# 4. Collect results (blocks up to --max-wait seconds, default 600)
audiogen --jobs jobs/classroom.json collect --max-wait 600

# 5. Copy origin_res/<game>/audio → <proj>/<game>/audio for Godot import
audiogen --jobs jobs/classroom.json install

# 6. Dry-run: validate jobs.json + print MiniMax envelopes (no network)
cargo run --bin dry_run -- jobs/classroom.json
```

## Jobs JSON schema

```json
{
  "game": "classroom",
  "style": "Chinese xianxia style baseline; prepended to every music prompt",
  "voice_id": "female-shaonv",
  "output_dir": "origin_res/classroom/audio",
  "install_dir": "classroom/classroom/audio",
  "jobs": [
    { "name": "bgm_lobby", "kind": "music",
      "prompt": "calm lobby music loop ...", "sample_rate": 32000,
      "channels": 2, "bitrate": 96000 },
    { "name": "voice_hint", "kind": "voice",
      "text": "再想想看", "voice_id": "female-shaonv", "speed": 0.95,
      "sample_rate": 24000, "channels": 1, "bitrate": 32000 },
    { "name": "sfx_click", "kind": "procedural",
      "synth": "woodblock", "duration_secs": 0.05,
      "sample_rate": 22050, "channels": 1, "bitrate": 64000 }
  ]
}
```

| `kind` | Required fields | Output format |
|---|---|---|
| `music` | `prompt` (instrumental description) | mp3 |
| `voice` | `text` (TTS), optional `voice_id`/`speed` | mp3 |
| `procedural` | `synth` (name), `duration_secs` | mp3 (default) / wav |

`synth` names: `woodblock` (UI click), `ding` (chime), `pluck` (string pluck),
`thud` (low drum), `fanfare` (triumph), `noise` (test).

## Adding a new game

1. Copy `jobs/classroom.json` to `jobs/<game>.json`, edit.
2. (Optional) Copy `inner_game_classroom/audio.rs` pattern into the new game's
   audio.rs and update boot_profile + ctrl trigger points.
3. Run `submit` from a machine that can reach the rustyme VPS.

## Adding a new codec

1. Add `src/codec/<fmt>.rs` implementing `Codec` trait (encode + decode).
2. Add a match arm in `codec::codec_for_ext`.
3. No other changes.

## v2 roadmap

- Pure-Rust OGG/Vorbis encoder (none mature today; skip unless needed).
- Pure-Rust AAC encoder (none production-quality today; ffmpeg/CoreAudio recommended).
- MP3 trim / normalize / sample-rate conversion (symphonia-backed).
- Resilient submit with retry / idempotency (use `Envelope::id` to dedup).

## Platform codec support matrix

Checked against each platform's native audio decoder. v1 coverages marked ✓; todo = stub.

|  Format  | Encode (pure Rust) | Decode | Android | iOS native | 微信小程序 | 抖音小程序 | WASM/Chrome | WASM/Safari |
|----------|--------------------|--------|---------|------------|-----------|-----------|-------------|-------------|
| **mp3**  | ✓ shine-rs         | ✓ symphonia | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| **wav**  | ✓ hand-rolled      | ✓ hand-rolled | ✓ | ✓ | ✓ | ✗ | ✓ | ✓ |
| **flac** | ✓ flacenc          | ✓ symphonia | ✓ (API 24+) | ✓ | ✗ | ✗ | ✓ | ✓ |
| **ogg**  | ✗ no pure-Rust enc | ✓ symphonia | ✓ | ✗ | ✗ | ✗ | ✓ | ✗ |
| **aac/m4a** | ✗ no pure-Rust enc | ✓ symphonia | ✓ | ✓ (preferred) | ✓ (preferred) | ✓ (preferred) | ✗ | ✓ |

**结论**：v1 的 **mp3 + wav** 已经覆盖 5 个目标场景全部；**flac** 加码后 Android 24+/iOS/WASM 有了无损选项。
**AAC 是 iOS/微信/抖音"首选"但非必须** — 这些平台都接受 mp3（文件略大，音质等价）。

**AAC/OGG 编码 stub 的原因**：pure-Rust 实现目前没有生产质量可用。生产推荐：
- AAC: macOS/iOS 用 `coreaudio-rs` + AudioToolbox，Android 用 NDK MediaCodec，Linux/服务端用 ffmpeg+libfdk-aac（LGPL）。
- OGG/Vorbis: ffmpeg+libvorbis；纯 Rust 仅实验性。

需要在 `audiogen` 端实现的话，加一个 `aac-ffmpeg` 子命令走 ffmpeg 子进程（破坏"无系统耦合"约束）或 platform-specific FFI 模块。

### Format selection per platform (建议 jobs.json)

```jsonc
// Android + WASM 全平台主用 mp3
{ "format": "mp3", "sample_rate": 32000, "bitrate": 96000 }

// iOS hi-fi BGM 用 flac
{ "format": "flac", "sample_rate": 44100 }

// 微信小程序开发期调试用 wav (无压缩，即时播放)
{ "format": "wav", "sample_rate": 22050 }
```

`audiogen` 启动后会自动 dispatch 到对应 codec，无需手动选。