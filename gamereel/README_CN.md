# gamereel

> 把游戏协议回放渲染成短视频。Rust + FFmpeg + CUDA。

`gamereel` 接收游戏的二进制协议消息（战报、对局结果、回放帧），渲染成 TikTok / 小红书 / IG Reels 形态的 MP4。每个支持的游戏自成一个 crate (`crates/proto-*`)，通过 `inventory::submit!` 自注册——加新游戏只需在 CLI 加一行依赖，引擎层零改动。

## 工作区结构

```
gamereel/
├── Cargo.toml                         # 工作区根
├── crates/
│   ├── gamereel-core/                 # 视频生成引擎 + ProtocolParser trait
│   ├── proto-puzzle/                  # 方块游戏协议解析（骨架）
│   └── proto-bubble/                  # 泡泡龙协议解析（骨架）
├── apps/
│   ├── gamereel-cli/                  # CLI 入口：`gamereel render --protocol …`
│   └── hs-mvp/                        # 原 demo（炉石风格战报）
├── benches/                           # mN.sh + results/mN.json 趋势数据
├── tools/quality-eval/                # VMAF + grid_search + scale_path_bench
└── docs/                              # optimization-log.md，设计记录
```

## 构建

```bash
cargo build --workspace --release       # 全部 crate
cargo test  --workspace                 # 全部测试（24 active + 8 CUDA-gated）
cargo bench -p gamereel-core            # criterion benches
cargo run   -p gamereel-cli -- list-protocols
```

**前置依赖：** ffmpeg 开发库（`libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev`）、`clang`、`pkg-config`。CUDA 全栈管线（M3+）：NVIDIA 驱动 ≥ 535 + `libnvrtc12` + `libnvrtc-builtins12.0`。质量基准：`vmaf`（Netflix libvmaf 3.x）+ `jq`。

## 加新游戏协议

1. 用 `proto-puzzle` 当模板，建 `crates/proto-<游戏名>/`。
2. 为你的类型实现 `ProtocolParser`，用 `inventory::submit!` 注册。
3. 在 `apps/gamereel-cli/Cargo.toml` 的 `[dependencies]` 加上新 crate，并在 `src/main.rs` 加 `use proto_<游戏名> as _;`（强制 link 防 inventory 构造器被剥光）。

跑 `gamereel list-protocols` 应该能看到你的新解析器。

## 性能演进（linux-nvenc-refactor 分支）

单路 720x1080 / 30 fps / 10 s，RTX 3060 + i7-13700K。两个数字关注：

  * **e2e fps** —— `perf_main` 5 次中位（合成 + 缩放 + 编码全链路）。
  * **encoder fps** —— 预解码源直接喂编码器，仅编码+缩放阶段。

| Milestone | Encoder | e2e fps (perf_main) | encoder fps | VMAF | 备注 |
|---|---|---:|---:|---:|---|
| **M0** | libx264 medium @ 6 Mbps | **152** | 535 (shell) / 152 (criterion) | 99.34 | 原硬编码 videotoolbox 在 Linux 直接 panic；用 libx264 测基线 |
| **M1** | h264_nvenc p4 balanced (auto) | **377 (2.48× M0)** | NVENC p4: 475, NVENC p2: **619**, libx264: 520 | 98.42 / 99.05 | 编码器自动选择、scaler 复用、z-order 确定性 |
| **M2** | EncoderProfile::Balanced (默认) | **381 (1.01× M1)** | Fast 474 / Balanced 462 / TikTokHQ 400 / IgReelsHDR 416 | Fast 97.87 / Balanced 97.73 / **TikTokHQ 97.48** / HDR 97.48 | 4 个 named profile + 144 点 VMAF 网格；e2e 持平因瓶颈转移——见 [O-011](docs/optimization-log.md#o-011) |
| **M3** | CUDA hwframes + h264_nvenc (cudarc kernel) | **456 (1.23× M2)** | (CUDA-only path) | 与 M2 一致 | 全 GPU 管线；cudarc RGBA→NV12 kernel；ffmpeg CUDA hwframes 池；**100 cycles 0 MB VRAM 泄漏** ([O-012..014](docs/optimization-log.md)) |
| M4 (target) | wgpu compositor + CUDA + NVENC | ≥ 1500 | ≥ 3000 | ≥ 95 | 替换 image_effect.rs CPU 合成 |
| M5 (target) | actix actor pool, batch-100 | 100 × 10 s ≤ 120 s wall | — | ≥ 95 | 硬件感知并发上限 |

**CPU 时间真正在哪**（M2 的 [`cpu_breakdown`](crates/gamereel-core/tests/cpu_breakdown.rs) 实测）：perf_main 场景下 RGBA→YUV 色彩转换（`sws_scale`，CPU SIMD）占 phase time **46%**；NVENC submit+wait **41%**；CPU 合成只占 **13%**。M3 干掉了 sws_scale 这块；M4 的 wgpu compositor 会干掉合成；M5 的 actor 池把吞吐扩到 NVENC 硬件天花板。

**取证纪律**：每一次性能改动在 [`docs/optimization-log.md`](docs/optimization-log.md) 都有条目，记录*假设*、*自证测试*、*实测增量*、以及*复盘*（说明假设和现实差距）。几个月后回查决策时直接读这个文件就够了。

## License

See LICENSE file.
