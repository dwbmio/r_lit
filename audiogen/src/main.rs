//! audiogen CLI — submit/collect/install/sfx for game audio pipelines.
//!
//! Subcommands:
//!   submit   — SSH-LPUSH jobs to VPS rustyme queue (async, returns immediately)
//!   collect  — SSH-BRPOP results from VPS, decode + land files
//!   sfx      — run procedural jobs locally (no network)
//!   install  — copy origin_res/<game>/audio → <proj>/<game>/audio
//!   status   — show .pending.json state

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use audiogen::jobs::JobsFile;
use audiogen::pipeline;
use audiogen::r2;
use audiogen::ssh;

#[derive(Parser, Debug)]
#[command(name = "audiogen", version, about = "Pure-Rust game audio pipeline")]
struct Cli {
    /// Path to jobs JSON file (e.g. jobs/classroom.json).
    #[arg(short, long)]
    jobs: PathBuf,

    /// Redis URL for direct LPUSH/BRPOP (preferred over SSH). Env: AUDIOGEN_REDIS_URL.
    #[arg(long, global = true, env = "AUDIOGEN_REDIS_URL")]
    redis_url: Option<String>,

    /// Override queue key. Env: AUDIOGEN_REDIS_QUEUE.
    #[arg(long, global = true, env = "AUDIOGEN_REDIS_QUEUE")]
    redis_queue: Option<String>,

    /// Override result key. Env: AUDIOGEN_REDIS_RESULT.
    #[arg(long, global = true, env = "AUDIOGEN_REDIS_RESULT")]
    redis_result: Option<String>,

    /// VPS host (default: rustyme deploy host). Can also come from AUDIOGEN_VPS env.
    #[arg(long, global = true, env = "AUDIOGEN_VPS", default_value = "root@152.32.210.127")]
    vps: String,

    /// Path to SSH private key. If unset, ssh-agent is used.
    #[arg(long, global = true, env = "AUDIOGEN_SSH_KEY")]
    ssh_key: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Submit AI jobs to VPS redis queue (async — does not wait for results).
    Submit {
        /// Comma-separated job names; default = all.
        #[arg(long)]
        only: Option<String>,
    },
    /// Collect results from VPS (BRPOP). Blocks up to --max-wait seconds.
    Collect {
        #[arg(long, default_value_t = 600)]
        max_wait: u64,
    },
    /// Run procedural SFX jobs locally (no VPS).
    Sfx {
        #[arg(long)]
        only: Option<String>,
    },
    /// Copy generated audio to the Godot project tree.
    Install,
    /// Upload generated audio to R2 AND register in HFrog (runtime + artifactory).
    Release {
        /// Public base URL for R2 (e.g. https://r2.gamesci-lite.com). Default: r2.gamesci-lite.com.
        #[arg(long, default_value = "https://r2.gamesci-lite.com")]
        public_base: String,
        /// HFrog API base URL.
        #[arg(long, default_value = audiogen::hfrog::DEFAULT_HFROG_BASE)]
        hfrog_base: String,
        /// Optional override of R2 credentials .s3 file path.
        #[arg(long)]
        s3_file: Option<std::path::PathBuf>,
        /// Optional: comma-separated job names to upload; default = all generated files.
        #[arg(long)]
        only: Option<String>,
        /// Version label for HFrog artifactory entries (default: 0.1.0).
        #[arg(long, default_value = "0.1.0")]
        ver: String,
        /// Skip HFrog registration (R2 only).
        #[arg(long)]
        skip_hfrog: bool,
    },
    /// Show pending state.
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let jf = JobsFile::load(&cli.jobs)
        .with_context(|| format!("loading jobs: {}", cli.jobs.display()))?;

    // Build connection — Redis preferred (no SSH required), SSH fallback.
    let redis = match &cli.redis_url {
        Some(url) => {
            let r = audiogen::redis_q::RedisQueue::new(
                url, cli.redis_queue.as_deref(), cli.redis_result.as_deref(),
            )?;
            eprintln!("redis: {} queue={} result={}", url, r.queue_key(), r.result_key());
            Some(r)
        }
        None => None,
    };
    let needs_ssh = redis.is_none() && matches!(cli.cmd, Cmd::Submit { .. } | Cmd::Collect { .. });
    let mut ssh_conn: Option<audiogen::ssh::SshClient> = if needs_ssh {
        Some(ssh::connect_host(&cli.vps, cli.ssh_key.as_deref()).await?)
    } else {
        None
    };

    match cli.cmd {
        Cmd::Submit { only } => {
            let only_vec = only.as_deref().map(split_csv);
            pipeline::submit(&jf, redis.as_ref(), ssh_conn.as_mut(), only_vec.as_deref()).await?;
        }
        Cmd::Collect { max_wait } => {
            pipeline::collect(&jf, redis.as_ref(), ssh_conn.as_mut(), max_wait).await?;
        }
        Cmd::Sfx { only } => {
            let only_vec = only.as_deref().map(split_csv);
            pipeline::run_procedural(&jf, only_vec.as_deref())?;
        }
        Cmd::Install => {
            pipeline::install(&jf)?;
        }
        Cmd::Release { public_base, hfrog_base, s3_file, only, ver, skip_hfrog } => {
            let (bucket, ak, sk, endpoint, region) =
                audiogen::r2::R2Client::load_env_or_file(s3_file.as_deref())?;
            let r2_client = audiogen::r2::R2Client::new(
                &bucket, &region, &endpoint, &ak, &sk, &public_base,
            )?;
            let hf_client = if skip_hfrog { None } else { Some(audiogen::hfrog::HfrogClient::new(&hfrog_base)) };
            cmd_release(&jf, &r2_client, hf_client.as_ref(), only.as_deref(), &ver).await?;
        }
        Cmd::Status => {
            let st = pipeline::load_pending(&std::path::PathBuf::from(&jf.output_dir), &jf.game)?;
            println!("game: {}", st.game);
            println!("last_submit_at: {:?}", st.last_submit_at);
            println!("pending ({}):", st.jobs.len());
            for (k, j) in &st.jobs {
                println!("  {}  kind={:?}  format={}  submitted_at={}",
                         k, j.kind, j.format, j.submitted_at);
            }
        }
    }
    Ok(())
}

fn split_csv(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
}

async fn cmd_release(
    jf: &JobsFile,
    r2_client: &audiogen::r2::R2Client,
    hf_client: Option<&audiogen::hfrog::HfrogClient>,
    only: Option<&str>,
    ver: &str,
) -> anyhow::Result<()> {
    let only_vec = only.map(split_csv);
    let dir = std::path::PathBuf::from(&jf.output_dir);
    if !dir.is_dir() {
        anyhow::bail!("output_dir does not exist: {}", dir.display());
    }
    // Step 1: ensure runtime exists (idempotent).
    if let Some(hf) = hf_client {
        let runtime = audiogen::hfrog::runtime_for_game(&jf.game, ver);
        let desc = format!(
            "Per-game runtime audio: BGM/SFX/voice generated by audiogen. Path: gamesci-<game>/<format>/<name>.<ext>"
        );
        hf.ensure_runtime(
            &runtime, ver,
            "audiogen game audio (mp3/wav/ogg)",
            &desc,
            &["mp3", "wav", "ogg"],
        ).await.with_context(|| format!("ensure runtime {runtime}"))?;
        println!("  ✓ HFrog runtime registered: {runtime}");
    }
    // Step 2: upload + register each file.
    let mut uploaded = 0;
    let mut registered = 0;
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let p = entry.path();
        if !p.is_file() { continue; }
        let stem = match p.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        let fmt = match p.extension().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if !["mp3","wav","ogg"].contains(&fmt) { continue; }
        if let Some(filter) = only_vec.as_ref() {
            if !filter.contains(&stem.to_string()) { continue; }
        }
        let key = audiogen::r2::default_key(&jf.game, fmt, stem);
        let ct = match fmt {
            "mp3" => "audio/mpeg",
            "wav" => "audio/wav",
            "ogg" => "audio/ogg",
            _ => "application/octet-stream",
        };
        // R2 upload.
        let res = r2_client.upload_file(&p, &key, ct).await?;
        println!("  ↑ {key}  ({res}B)  → {ct}", res = res.bytes);
        uploaded += 1;
        // HFrog registration.
        if let Some(hf) = hf_client {
            let runtime = audiogen::hfrog::runtime_for_game(&jf.game, ver);
            let md5 = audiogen::hfrog::md5_hex_file(&p)?;
            let desc = format!("audiogen: {} {} ({})", jf.game, stem, fmt);
            hf.register_file(
                &runtime, stem, ver, &key, res.bytes, &md5, &desc,
            ).await.with_context(|| format!("register {key}"))?;
            println!("     ✓ HFrog: pid=? name={stem} runtime={runtime}");
            registered += 1;
        }
    }
    println!("== uploaded {uploaded} files to R2, registered {registered} in HFrog ==");
    Ok(())
}