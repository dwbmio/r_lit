//! `down100-replay-renderer` — Phase 7.8 Replay 队列消费者 + mp4 渲染管道.
//!
//! 替代 `tools/down100-replay-render/compose_scene.py` + `perf_main` 的 Python 组合,
//! 走 Rust 一次性闭环 (无 Python 解析延迟, 无 cargo run 重启开销).
//!
//! ## 工作流
//!
//! ```text
//! loop {
//!     1. BRPOP rustyme:titan:write  (200ms 阻塞超时)
//!     2. 解 envelope JSON, 过滤 op="render_replay"
//!     3. proto_down100::decode_payload → snapshots + events + rosters
//!     4. proto_down100::translate → MetaSceneList + 写 PNG 资源到 tmp dir
//!     5. gamereel-core StageMgr::start_gen_first → mp4 (默认 NVENC, fallback libx264)
//!     6. mv 到 OUTPUT_DIR/<room_id>.mp4
//! }
//! ```
//!
//! ## 启动
//!
//! ```bash
//! down100-replay-renderer \
//!   --redis-url redis://127.0.0.1:6379 \
//!   --queue rustyme:titan:write \
//!   --output-dir /tmp/down100-mp4 \
//!   --once        # 处理一条就退出 (CI / smoke 用); 不带 --once 则常驻
//! ```
//!
//! ## env vars (透传给 gamereel-core 编码层)
//!
//! - `GAMEREEL_FORCE_SW=1` → 强制 libx264 (调试 / GPU 不可用)
//! - `GAMEREEL_DURATION_SECS=N` → 覆盖渲染时长 (默认按 translate 估算)
//! - `RUST_LOG=info,down100_replay_renderer=debug` → 日志级别

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use gamereel_core::ffmpeg_inc::stage_mgr::StageMgr;
use gamereel_core::{ffmpeg_inc, RuntimeCtx};
use proto_down100::translate::{CANVAS_H, CANVAS_W, FPS};
use proto_down100::{assets, decode_payload, translate};
use redis::AsyncCommands;
use tempfile::TempDir;

#[derive(Parser, Debug)]
#[command(
    name = "down100-replay-renderer",
    version,
    about = "Consumes titan-forge Phase 7.8 Replay fanout → renders mp4 via gamereel-core."
)]
struct Cli {
    /// Redis 连接串 (跟 titan-forge [task_queue].redis_url 对齐)
    #[arg(long, default_value = "redis://127.0.0.1:6379")]
    redis_url: String,
    /// Redis 队列 key (跟 [task_queue].queue_key 对齐)
    #[arg(long, default_value = "rustyme:titan:write")]
    queue: String,
    /// mp4 输出目录 — 自动创建. mp4 命名为 <room_id>.mp4
    #[arg(long, default_value = "/tmp/down100-mp4")]
    output_dir: PathBuf,
    /// 只处理一条 envelope 就退出 (CI / smoke / debug 用). 默认常驻 brpop.
    #[arg(long)]
    once: bool,
    /// LINDEX 第 0 条而不是 RPOP (调试: 不消费 queue, 同一条反复测)
    #[arg(long)]
    peek: bool,
    /// brpop 阻塞超时 (秒). 0 = 永久等. 默认 5 秒 (方便 SIGINT 响应).
    #[arg(long, default_value_t = 5)]
    brpop_timeout_secs: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,down100_replay_renderer=debug"),
    )
    .init();
    ffmpeg_inc::init_env().context("ffmpeg init_env")?;

    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.output_dir)
        .with_context(|| format!("create output dir {:?}", cli.output_dir))?;

    log::info!(
        "down100-replay-renderer started — redis={} queue={} output={:?} once={} peek={}",
        mask_redis_url(&cli.redis_url),
        cli.queue,
        cli.output_dir,
        cli.once,
        cli.peek,
    );

    let client = redis::Client::open(cli.redis_url.as_str())?;
    let mut conn = redis::aio::ConnectionManager::new(client).await?;

    loop {
        let raw: Option<String> = if cli.peek {
            conn.lindex::<_, Option<String>>(&cli.queue, 0).await?
        } else {
            // brpop 返回 (queue_name, value)
            let res: Option<(String, String)> = conn
                .brpop(&cli.queue, cli.brpop_timeout_secs as f64)
                .await?;
            res.map(|(_q, v)| v)
        };

        match raw {
            Some(envelope_json) => match handle_one(&envelope_json, &cli.output_dir).await {
                Ok(mp4_path) => {
                    log::info!("rendered → {}", mp4_path.display());
                    if cli.once {
                        return Ok(());
                    }
                }
                Err(e) => {
                    log::error!("render failed: {e:#}");
                    if cli.once {
                        return Err(e);
                    }
                }
            },
            None => {
                if cli.once {
                    log::warn!("queue empty, exiting (--once)");
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
}

async fn handle_one(envelope_json: &str, output_dir: &PathBuf) -> Result<PathBuf> {
    let t0 = std::time::Instant::now();
    let env: serde_json::Value = serde_json::from_str(envelope_json)
        .context("parse envelope JSON")?;
    let op = env
        .get("kwargs")
        .and_then(|k| k.get("op"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if op != "render_replay" {
        return Err(anyhow!("skip op={op}"));
    }
    let payload = env
        .get("kwargs")
        .and_then(|k| k.get("payload"))
        .ok_or_else(|| anyhow!("missing kwargs.payload"))?;

    let room_id = payload
        .get("room_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    log::debug!("envelope room={room_id}, decoding zstd_blob...");
    let decoded = decode_payload(payload).context("decode_payload")?;
    log::info!(
        "decoded room={} snapshots={} events={} rosters={}",
        decoded.header.room_id,
        decoded.snapshots.len(),
        decoded.events.len(),
        decoded.rosters.len(),
    );

    let trans = translate(&decoded);
    if trans.scene_list.meta_scene_list.is_empty() {
        return Err(anyhow!("translate produced empty scene (no snapshots)"));
    }
    log::info!(
        "translated nodes={} duration_secs={:.2} players={}",
        trans
            .scene_list
            .meta_scene_list
            .first()
            .map(|s| s.nodes.len())
            .unwrap_or(0),
        trans.duration_secs,
        trans.player_count,
    );

    // 写 assets 到 tmp dir (RuntimeCtx 从这里加载 PNG)
    let tmp = TempDir::new().context("temp dir")?;
    assets::write_assets(tmp.path(), &trans.room_id, &trans.player_ids)
        .context("write assets")?;
    log::debug!("assets written to {:?}", tmp.path());

    // 计算 RuntimeCtx duration — env override 优先, 否则 translate 估算
    let dur_secs: u64 = std::env::var("GAMEREEL_DURATION_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| trans.duration_secs.ceil() as u64 + 1);

    let mut rtx = RuntimeCtx::new(CANVAS_W, CANVAS_H, dur_secs, FPS);
    rtx.set_source_path(tmp.path().to_path_buf());
    let mut stage = StageMgr::new(trans.scene_list);
    stage.meta_scene_preload(&mut rtx, 0).context("preload")?;

    let out_path = output_dir.join(format!("{}.mp4", sanitize(&room_id)));
    stage
        .start_gen_first(&mut rtx, &out_path)
        .context("start_gen_first")?;

    let elapsed_ms = t0.elapsed().as_millis();
    log::info!(
        "render done room={} dur={}s -> {} ({} ms)",
        room_id,
        dur_secs,
        out_path.display(),
        elapsed_ms
    );
    Ok(out_path)
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn mask_redis_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((userinfo, hostpart)) = rest.split_once('@') {
            if let Some((user, _pass)) = userinfo.split_once(':') {
                return format!("{scheme}://{user}:***@{hostpart}");
            }
            return format!("{scheme}://***@{hostpart}");
        }
    }
    url.to_string()
}
