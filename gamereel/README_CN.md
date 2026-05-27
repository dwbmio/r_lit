# gamereel

> 把游戏协议回放渲染成短视频。Rust + FFmpeg + CUDA。

`gamereel` 接收游戏的二进制协议消息（战报、对局结果、回放帧），渲染成 TikTok / 小红书 / IG Reels 形态的 MP4。每个支持的游戏自成一个 crate (`crates/proto-*`)，通过 `inventory::submit!` 自注册——加新游戏只需在 CLI 加一行依赖，引擎层零改动。

---

## 实测能力（基于实际数据，不基于承诺）

下面所有数字都在**实际 hs-mvp 场景**（720×1080 × 30 fps × 10 s，6 个 node 含时间线动画）上测得，参考机器：**NVIDIA RTX 3060 + Intel i7-13700K + Ubuntu 24.04**。复现命令：`cargo run --release -p hs-mvp --bin farm_bench -- --jobs 100 --workers 1,2,3,4,6,8`。

### ✅ 优势

| 能力 | 实测 |
|---|---|
| **单视频亚 300ms 延迟**（单 worker） | p50 284 ms / p99 290 ms — 满足准实时 SLA |
| **220 视频/分钟吞吐**（2 workers） | 100 个 hs-mvp 视频 **26.9 秒墙钟**完成 |
| **消费级 GPU 性价比** | RTX 3060（约 ¥2200）跑出 1113 fps |
| **平台级码率下的画质** | VMAF 97.5+ across `Fast/Balanced/TikTokHQ`，144 点 grid 校准 |
| **长时间运行无 VRAM 增长** | 100 次连跑后显存差 0 MB ([cuda_vram_leak](crates/gamereel-core/tests/cuda_vram_leak.rs)) |
| **输出确定性** | 同场景跨运行 RGBA 字节级 hash 一致 ([zorder_stable](crates/gamereel-core/tests/zorder_stable.rs)) |
| **可插拔游戏协议** | 加 `crates/proto-<游戏>/` + CLI 一行 dep；`inventory::submit!` 自注册 |
| **云端就绪** | `Worker` trait 抽象 + `RemoteWorker` 桩已落地，加 gRPC dispatch 不动主体逻辑 |
| **Rust 安全无泄漏** | 30 个 active 测试 + 10 个 CUDA-gated，生产路径无 `unwrap()` / `panic!()` |

### ⚠️ 已知限制（同样实测）

| 限制 | 原因 | 解法 |
|---|---|---|
| **单 NVENC 引擎天花板：worker > 2 不增吞吐** | RTX 3060 只有 1 个 NVENC 引擎，2 路并发已饱和 | 换硬件（4070 Ti Super 起 2× NVENC）或做 M4 降低单流成本 |
| **CPU 合成占帧时间约 40 %** | `image_effect.rs` 在 CPU 做 per-pixel alpha blend | M4 wgpu compositor（预期 +50–80% e2e） |
| **CUDA 路径每帧 `synchronize()`** | 杀掉 NVENC 异步流水线（约 14% 开销） | stream-aware NVENC 提交（M4 一并解决） |
| **每个 worker 进程 284 ms 一次性 CUDA init** | NVRTC kernel 编译 + ffmpeg hwframes 池建立 | M5 worker pool 已摊销，仅在每 job 起独立进程时显眼 |
| **单 GPU 限定** | 没做多 GPU 调度 | trait 抽象就位；多 GPU `LocalWorkerPool` 是机械工作 |
| **HDR profile 是桩** | `IgReelsHDR` 落到 `TikTokHQ` H.264 SDR | 真 HDR (HEVC Main10 + BT.2020) 是 M4+ 范围 |
| **没有 AV1 编码器** | RTX 3060 NVENC 第 7 代不带 AV1 | 硬件升级（RTX 40/50 系列 Ada/Blackwell） |
| **协议解析是骨架** | `proto-puzzle` / `proto-bubble` 只注册占位数据 | 真 binary decoder 是每游戏独立工作，刻意分开 |

---

## 性能演进（M0 → M5 实测）

同一硬件全程。

| Milestone | 改了什么 | e2e fps (perf_main) |
|---|---|---:|
| **M0 baseline** | libx264 medium @ 6 Mbps（Linux fallback 路径） | 152 |
| **M1** | 编码器自动选 (NVENC) + scaler 复用 + z-order BTreeMap | 377 (2.48×) |
| **M2** | 4 个 EncoderProfile + 144 点 VMAF grid | 381 (1.01×) |
| **M3** | 全 GPU 管线（cudarc kernel + ffmpeg CUDA hwframes） | 456 (1.23×) |
| **M5 (workers=1)** | LocalWorker 跨 jobs 持久 CUDA + ffmpeg context | **1004 fps**, p99 290 ms |
| **M5 (workers=2)** | WorkerPool round-robin 调度 | **1113 fps**, p99 547 ms |

**总体：152 → 1113 fps = 7.3× 基线，画质同步保持（VMAF 97.5+）**。

100 视频跑分：

| Workers | Wall (s) | Throughput (fps) | videos/min | p50 (ms) | p99 (ms) |
|---:|---:|---:|---:|---:|---:|
| 1 | 29.9 | 1004 | 201 | **284** | **290** |
| **2** | **26.9** | **1113** | **223** | 518 | 547 |
| 3 | 27.1 | 1107 | 221 | 781 | 810 |
| 4 | 27.4 | 1095 | 219 | 1047 | 1101 |
| 6 | 27.8 | 1078 | 216 | 1583 | 1695 |
| 8 | 28.3 | 1062 | 212 | 2112 | 2329 |

**workers=1 是延迟最优配置**（亚 300ms p99）。**workers=2 是吞吐最优**（+10% 吞吐，p99 翻倍）。超过 2 只是排队——吞吐持平，p99 单调爆炸。

---

## 扩展路线和瓶颈识别

按优先级列出阻碍 `gamereel` 在同机器上跑得更快的因素：

### Tier 1 — 软件优化，单 GPU（M4 范围）

| # | 瓶颈 | 解法 | 预期提升 |
|---|---|---|---|
| 1 | CPU `Scene::on_render`（约 40% 帧时间） | M4 wgpu compositor（干掉 image_effect.rs） | 单流 +50–80% |
| 2 | CUDA 路径每帧 `synchronize()` | stream-aware NVENC submit（cudarc + ffmpeg 共享 CUstream） | 单流 +10–15% |
| 3 | `cuMemcpy2D` 从 cudarc 自有 buffer 到 ffmpeg pool | kernel 直接写 pool device pointer（cudarc 0.20+ API） | +1–2%（修边边角角） |
| 4 | 每帧 `to_rgba8()` 转换 | 合成器之后 buffer 直接保持 `Vec<u8>` | +5% |

### Tier 2 — 硬件天花板（NVENC 引擎数）

RTX 3060 单 NVENC 引擎 = 我们 1113 fps 的天花板。**软件再优化无法突破**，要换显卡：

| GPU | NVENC 引擎数 | 估计 worker 峰值 | 备注 |
|---|---:|---:|---|
| RTX 4070 Ti Super 16GB | 2 | ~4 | ~¥6500；2× NVENC + 16GB VRAM（兼顾 AI 副业） |
| RTX 4080 Super 16GB | 2 | ~4 | ~¥9500；NVENC 数同 4070TiS，核心更强，AI 更猛 |
| RTX 4090 24GB | 2 | ~4 | ~¥14000；24GB 解锁 30B LLM Q4 + 同样 NVENC |
| RTX 5090 32GB | 3 | ~8 | ~¥18000；新 Blackwell，第 3 个 NVENC 引擎，32GB |
| **NVIDIA L4 24GB** | **4** | **~12** | ~¥18000 服务器渠道；72W 单槽；专为转码农场设计 |
| L40S 48GB | 3 | ~10 | ~¥60000；数据中心级；AI + 视频双修 |

L4 (4× NVENC) 预测：约 4400 fps（4× 当前峰值），100 视频约 7 秒。

### Tier 3 — 多 GPU

`Worker` trait 已抽象 dispatch。加 `MultiGpuWorkerPool` 在 N 个本地 GPU 间 round-robin 是机械改造。预期 4 GPU 内近线性扩展（再往上 PCIe / host RAM 带宽成新瓶颈）。

### Tier 4 — 云端 GPU（RemoteWorker via gRPC）

`RemoteWorker` 桩今天已经在仓库。补完包括：
1. cloud node 上跑 gamereel-farm-server 二进制（内部复用 `LocalWorker`）。
2. `crates/gamereel-farm/src/worker/remote.rs` 实现 gRPC `Render(RenderJob) → RenderResult` 调用。
3. 输出传输：流式回传 MP4 字节，OR 让 cloud node 直接上传调用方指定的对象存储 URL（生产推荐——避免视频回传 dispatcher）。

瓶颈转到：dispatcher ↔ cloud node 之间网络带宽。我们 720×1080 H.264 默认码率下，10 秒片输出约 60 KB——可忽略。

### Tier 5 — 高级格式（HDR + AV1）

硬件门控：
- **HDR (HEVC Main10 + BT.2020)** 需要 Ada (RTX 40) 或更新硬件 HW 编码。
- **AV1 编码** 需要 Ada 或更新（NVENC Gen 8+）。

这俩是 TikTok / IG 高级上传通道——平台二次压缩对 AV1/HDR 友好，画质有可见提升。

---

## Compositor 选择决策表：CPU vs wgpu

wgpu compositor **默认关闭**，opt-in 通过 `GAMEREEL_WORKER_COMPOSITOR=wgpu`。默认 CPU 是因为 hs-mvp 这类稀疏更新场景的 dirty cache 让 CPU 合成几乎免费。

**break-even 经验值（RTX 3060 实测）**：
- CPU image_effect 成本 ≈ 1 ns/像素 × 当帧实际触及的像素数
- wgpu compose+readback 成本 ≈ 0.5 ms/帧，常数
- 临界点 ≈ **每帧触及 50 万像素**（已扣除 dirty cache 命中部分）。低于这数 CPU 赢，高于这数 wgpu 赢 1.5–100×
- 实测数据：[`wgpu_break_even_sweep.rs`](crates/gamereel-compositor/tests/wgpu_break_even_sweep.rs) 显示 **N=1 个全屏 overlay**（无缓存帮助时）wgpu 已经赢 37–93×

### 留默认 CPU

| 场景类型 | 原因 | 游戏举例 |
|---|---|---|
| 稀疏 UI + 小静态背景 | dirty cache 命中 70-90%，几乎免费 | hs-mvp 数据卡、放置游戏战报 |
| Match-3 / 消除类回放（小格子）| 每格 32-96 px；64 格 × 4 KB = 25 万像素/帧，一半还命中 cache | 方块游戏战报、消除类回放 |
| 卡牌动画（1 背景 + ≤10 张卡）| 只有卡是 dirty，背景缓存不变 | 炉石战报、MTG/PvP 卡牌回放 |
| 一次性结算 / 计分页 + 滚动文字 | 文字是唯一动画，背景缓存 | 周报、排行榜开场 |
| 谈话头肩回放（脸 + 滚动弹幕）| 小区域 overlay，大背景缓存 | 直播风战报 |

### 切到 wgpu (`GAMEREEL_WORKER_COMPOSITOR=wgpu`)

| 场景类型 | 原因 | 游戏举例 |
|---|---|---|
| 含全屏 VFX 的战斗回放（爆炸/抖屏/调色闪）| 每帧整画布刷一遍，cache 完全失效 | RPG/MOBA 战斗高光、塔防波次 |
| 视差/分层全屏 cinematic 开场 | 3-5 层全屏每帧都动 | 二次元 ARPG 开场、赛季 PV |
| 动态背景每帧重渲染 | `_clear_image` 缓存失效 | 实时 tiling 背景、动态天气 |
| 粒子密集（50+ 全屏粒子）| 粒子覆盖整个画布 | 弹幕射击、shoot-em-up 回放 |
| 全屏滤镜（模糊、调色、bloom）| 每帧每像素都过滤镜 | 梦境/回忆闪回转场 |
| 棋盘缩放 / 旋转转场 | 整画布几何变换 | 棋盘游戏缩放、MOBA 全图扫场 |

### 不确定怎么办：套公式

```
expected_pixels_per_frame =
   sum(sprite_w * sprite_h * (1 if 该 sprite 每帧都变 else 0))

if expected_pixels_per_frame > 500_000:
    GAMEREEL_WORKER_COMPOSITOR=wgpu
else:
    保持默认 (CPU)
```

720×1080 下 50 万像素 ≈ **3 个 400×400 的 sprite 同时全帧变动**，或 **1 个全屏背景每帧重画**。只要**单个全屏图层每帧都需要重渲染**，就过 break-even，切 wgpu。

---

## 工作区结构

```
gamereel/
├── Cargo.toml                         # 工作区根
└── crates/
    ├── gamereel-core/                 # 视频生成引擎 + ProtocolParser trait + perf_main bin
    ├── gamereel-compositor/           # wgpu compositor (M4) — opt-in 重场景路径
    ├── gamereel-farm/                 # worker pool + 硬件 probe + Worker trait
    ├── gamereel-output/               # OutputSink trait + LocalDiskSink + ObjectStorageSink
    ├── proto-puzzle/                  # 方块游戏 v0 JSON 解析 + Scene 翻译
    └── proto-bubble/                  # 泡泡龙协议解析（骨架）
```

## 构建

```bash
cargo build --workspace --release
cargo test  --workspace                 # 30 active + 10 CUDA-gated
cargo run   -p gamereel-core --bin perf_main --release
```

**前置依赖：** ffmpeg 开发库（`libavcodec-dev libavformat-dev libavfilter-dev libavutil-dev libswscale-dev`）、`clang`、`pkg-config`。CUDA 全栈：NVIDIA 驱动 ≥ 535、`libnvrtc12`、`libnvrtc-builtins12.0`。

## 加新游戏协议

1. `cp -r crates/proto-puzzle crates/proto-<游戏名>` 然后改 Cargo.toml 包名。
2. 给你的类型实现 `ProtocolParser`，用 `inventory::submit!` 注册。
3. 在你的消费 crate 中加 `proto-<游戏名>` dep + `use proto_<游戏名> as _;`（强制 link 防 `lto=fat` 剥光 inventory 构造器）。

## 取证纪律

每一次性能改动在 commit message 中记录 *假设*、*自证测试*、*实测增量*、*复盘*。几个月后回查决策时直接读 git log。

## License

See LICENSE file.
