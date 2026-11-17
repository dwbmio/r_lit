# audiogen — Pure-Rust game audio pipeline

Pure-Rust CLI for game audio assets. Replaces one-off per-game Python scripts with a single JSON-driven tool and a multi-format codec trait.

## Features

- **Multi-format codec trait** (`Mp3Codec`, `WavCodec`, `FlacCodec`, `OggCodec`) — one impl per format, easy to extend.
- **MiniMax async pipeline** — `submit` (SSH-LPUSH to VPS rustyme Redis queue) returns immediately; `collect` (BRPOP) fetches results when ready. Pending state is tracked in `origin_res/<game>/audio/.pending.json`.
- **Procedural SFX** — 6 deterministic generators (`woodblock` / `ding` / `pluck` / `thud` / `fanfare` / `noise`), zero-cost and offline.
- **Pure Rust, no system coupling** — no ffmpeg / no native libs. `shine-rs` encodes MP3, `flacenc` encodes FLAC, hand-rolled WAV codec, `symphonia` decodes, `russh` handles SSH.

## Crate Dependencies

`clap`, `serde`, `serde_json`, `anyhow`, `thiserror`, `shine-rs`, `flacenc`, `symphonia`, `russh`, `russh-keys`, `tokio`, `futures`, `base64`

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

# 5. Copy origin_res/<game>/audio -> <proj>/<game>/audio for Godot import
audiogen --jobs jobs/classroom.json install

# 6. Dry-run: validate jobs.json + print MiniMax envelopes (no network)
cargo run --bin dry_run -- jobs/classroom.json
```

## Jobs JSON Schema

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
      "text": "Think again", "voice_id": "female-shaonv", "speed": 0.95,
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

`synth` names: `woodblock` (UI click), `ding` (chime), `pluck` (string pluck), `thud` (low drum), `fanfare` (triumph), `noise` (test).

## Adding a New Game

1. Copy `jobs/classroom.json` to `jobs/<game>.json` and edit.
2. Run `submit` from a machine that can reach the rustyme VPS.

## Adding a New Codec

1. Add `src/codec/<fmt>.rs` implementing the `Codec` trait (encode + decode).
2. Add a match arm in `codec::codec_for_ext`.

## v2 Roadmap

- Pure-Rust OGG/Vorbis encoder (none mature today; skip unless needed).
- Pure-Rust AAC encoder (none production-quality today; ffmpeg/CoreAudio recommended).
- MP3 trim / normalize / sample-rate conversion (`symphonia`-backed).
- Resilient submit with retry / idempotency (use `Envelope::id` to dedup).

## Platform Codec Support Matrix

| Format | Encode (pure Rust) | Decode | Android | iOS native | WeChat Mini Program | Douyin Mini Program | WASM/Chrome | WASM/Safari |
|---|---|---|---|---|---|---|---|---|
| **mp3** | shine-rs | symphonia | Yes | Yes | Yes | Yes | Yes | Yes |
| **wav** | hand-rolled | hand-rolled | Yes | Yes | Yes | No | Yes | Yes |
| **flac** | flacenc | symphonia | Yes (API 24+) | Yes | No | No | Yes | Yes |
| **ogg** | stub | symphonia | Yes | No | No | No | Yes | No |
| **aac/m4a** | stub | symphonia | Yes | Preferred | Preferred | Preferred | No | Yes |

**Takeaway**: `mp3 + wav` already cover all target platforms; `flac` adds lossless support for Android 24+, iOS, and WASM. While AAC is preferred on iOS / mini-programs, all of them accept MP3 with equivalent perceptual quality.

### Format Selection per Platform (`jobs.json`)

```jsonc
// Android + WASM primary format: mp3
{ "format": "mp3", "sample_rate": 32000, "bitrate": 96000 }

// iOS hi-fi BGM: flac
{ "format": "flac", "sample_rate": 44100 }

// WeChat Mini Program dev debugging: wav (uncompressed, instant playback)
{ "format": "wav", "sample_rate": 22050 }
```

`audiogen` dispatches to the matching codec automatically based on the target format.
