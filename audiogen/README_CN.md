# audiogen — 纯 Rust 游戏音频管线

纯 Rust 游戏音频资产 CLI，用单工具 + JSON 任务配置 + 多格式 Codec trait 替代按游戏手写的 Python 脚本。

## 特性

- **多格式 Codec trait**（`Mp3Codec`、`WavCodec`、`FlacCodec`、`OggCodec`）：每种格式一个实现，新增格式只需加一个文件。
- **MiniMax 异步管线**：`submit` 通过 SSH 向远端 Redis 队列推送任务并立即返回，`collect` 通过 `BRPOP` 拉取结果；状态落盘在 `origin_res/<game>/audio/.pending.json`。
- **程序化音效**：内置 6 种确定性合成器（`woodblock` / `ding` / `pluck` / `thud` / `fanfare` / `noise`），零成本离线生成。
- **纯 Rust 无系统耦合**：不依赖 ffmpeg 或系统原生库（`shine-rs` 编 MP3、`flacenc` 编 FLAC、手写 WAV、`symphonia` 解码、`russh` 走 SSH）。

## 常用命令

```bash
# 查看任务状态
audiogen --jobs jobs/classroom.json status

# 生成程序化音效（本地离线）
audiogen --jobs jobs/classroom.json sfx
audiogen --jobs jobs/classroom.json sfx --only sfx_click,sfx_chain

# 提交 AI 音频任务到 VPS
audiogen --jobs jobs/classroom.json submit
AUDIOGEN_VPS=root@vps.example.com AUDIOGEN_SSH_KEY=~/.ssh/id_rsa \
    audiogen --jobs jobs/classroom.json submit --only bgm_lobby

# 拉取生成结果（默认最多等待 600 秒）
audiogen --jobs jobs/classroom.json collect --max-wait 600

# 安装到游戏工程目录
audiogen --jobs jobs/classroom.json install

# 本地干跑校验 jobs.json
cargo run --bin dry_run -- jobs/classroom.json
```

## 任务配置（`jobs.json`）

| `kind` | 必填字段 | 输出格式 |
|---|---|---|
| `music` | `prompt`（器乐描述） | mp3 |
| `voice` | `text`（TTS 文本），可选 `voice_id`/`speed` | mp3 |
| `procedural` | `synth`（合成器名）、`duration_secs` | mp3（默认） / wav |

`synth` 可选：`woodblock`、`ding`、`pluck`、`thud`、`fanfare`、`noise`。

## 平台格式选型建议

| 格式 | 纯 Rust 编码 | 解码 | Android | iOS | 微信小程序 | 抖音小程序 | WASM/Chrome | WASM/Safari |
|---|---|---|---|---|---|---|---|---|
| **mp3** | `shine-rs` | `symphonia` | 支持 | 支持 | 支持 | 支持 | 支持 | 支持 |
| **wav** | 手写 | 手写 | 支持 | 支持 | 支持 | 不支持 | 支持 | 支持 |
| **flac** | `flacenc` | `symphonia` | 支持 (API 24+) | 支持 | 不支持 | 不支持 | 支持 | 支持 |
| **ogg** | 未实现 | `symphonia` | 支持 | 不支持 | 不支持 | 不支持 | 支持 | 不支持 |
| **aac/m4a** | 未实现 | `symphonia` | 支持 | 首选 | 首选 | 首选 | 不支持 | 支持 |

**结论**：`mp3 + wav` 已覆盖全部目标平台，`flac` 补充无损选项；iOS 与小游戏虽然首选 AAC，但均兼容 MP3。
