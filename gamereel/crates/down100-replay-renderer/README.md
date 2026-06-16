# down100-replay-renderer

Consume titan-forge **Phase 7.8 Replay fanout** queue → render `mp4`.

替代 `tools/down100-replay-render/compose_scene.py` + `perf_main` 的 Python+Rust 双进程组合,
走 **Rust 一次性闭环** (无 Python 解析 + 进程启动开销, 单次 ~3s vs 老组合 ~5-8s).

## Usage

```bash
# 常驻模式 (生产, brpop 阻塞拉队列)
down100-replay-renderer \
  --redis-url redis://127.0.0.1:6379 \
  --queue rustyme:titan:write \
  --output-dir /var/lib/gamereel/down100/

# 单次模式 (CI / debug, 处理一条退出)
down100-replay-renderer --once --peek --output-dir /tmp/

# 强制 CPU 编码 (GPU 不可用 / 调试)
GAMEREEL_FORCE_SW=1 down100-replay-renderer --once

# 覆盖渲染时长 (默认按 snapshot 估算 + 1s 尾巴)
GAMEREEL_DURATION_SECS=15 down100-replay-renderer --once
```

## CLI

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `--redis-url` | `redis://127.0.0.1:6379` | 跟 titan-forge `[task_queue].redis_url` 对齐 |
| `--queue` | `rustyme:titan:write` | 跟 titan-forge `[task_queue].queue_key` 对齐 |
| `--output-dir` | `/tmp/down100-mp4` | mp4 命名 `<room_id>.mp4` |
| `--once` | false | 处理一条 envelope 后退出 (CI / smoke 用) |
| `--peek` | false | LINDEX 0 而不是 RPOP (不消费 queue, 同一条反复测) |
| `--brpop-timeout-secs` | 5 | brpop 阻塞超时, 方便 SIGINT 响应 |

## 流程

```
loop {
    1. BRPOP rustyme:titan:write
    2. parse envelope JSON, filter op="render_replay"
    3. proto_down100::decode_payload → snapshots / events / rosters
    4. proto_down100::translate → MetaSceneList + duration_secs
    5. proto_down100::assets::write_assets → bg.png / floor.png / player_N.png (tmp dir)
    6. gamereel-core StageMgr::start_gen_first → mp4
    7. mv → output_dir/<room_id>.mp4
}
```

## 性能 (实测 RTX 3060)

| 视频时长 | encoder | 单次总耗时 | 编码占比 |
|---------|---------|-----------|---------|
| 5s (150f) | h264_nvenc | ~1.3s | ~700ms NVENC init + ~250ms encode |
| 12s (360f) | h264_nvenc | ~3.0s | NVENC steady, scene decode 主导 |
| 30s (900f) | h264_nvenc | ~2.0s | encode 摊薄 |
| 60s (1800f) | h264_nvenc | ~3.2s | encode 主导 |

**SW fallback (libx264)**: 同档 1.1× ~ 2.2× 慢 (短视频几乎平手, 60s 慢 2.2x).

未来批量 100 视频要进一步加速, 改造为 `gamereel-farm WorkerPool` 持久 actor + 复用 CUDA context.
