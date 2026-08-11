//! End-to-end pipeline: submit jobs to VPS redis queue, collect results.
//!
//! v1 limits:
//!   - Output format defaults to MP3 (MiniMax returns MP3; procedural uses shine-rs).
//!   - No MP3 trim / normalize / resample. AI results land as-is.
//!   - `.pending.json` lives next to the output dir to track in-flight jobs.

use crate::codec::{codec_for_ext, EncodeCfg};
use crate::jobs::{JobKind, JobsFile};
use crate::redis_q::RedisQueue;
use crate::synth;
use crate::ssh::SshClient;
use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const QUEUE: &str = "rustyme:audgen-minimax:queue";
pub const RESULT: &str = "rustyme:audgen-minimax:result";

/// `.pending.json` schema — tracked per-game on the **local** machine.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PendingState {
    pub game: String,
    pub jobs: HashMap<String, PendingJob>,
    /// Unix timestamp of last submit run (for staleness checks).
    pub last_submit_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingJob {
    pub cache_key: String,
    pub kind: JobKind,
    pub format: String,
    pub submitted_at: u64,
}

/// Load or initialize pending state for a jobs file.
pub fn load_pending(output_dir: &Path, game: &str) -> Result<PendingState> {
    let path = output_dir.join(".pending.json");
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        Ok(serde_json::from_str(&raw).unwrap_or_else(|_| PendingState {
            game: game.to_string(),
            ..Default::default()
        }))
    } else {
        Ok(PendingState { game: game.to_string(), ..Default::default() })
    }
}

pub fn save_pending(output_dir: &Path, st: &PendingState) -> Result<()> {
    std::fs::create_dir_all(output_dir)?;
    let path = output_dir.join(".pending.json");
    let raw = serde_json::to_string_pretty(st)?;
    std::fs::write(&path, raw)?;
    Ok(())
}

/// Build the rustyme envelope for one job and LPUSH it via direct Redis (preferred)
/// or SSH fallback if `redis` is None. Returns the cache_key for tracking.
pub async fn submit_one_redis(
    redis: Option<&RedisQueue>,
    game: &str,
    name: &str,
    kwargs: serde_json::Value,
) -> Result<String> {
    let cache_key = format!("{game}/audio/{name}");
    let body = build_envelope(game, name, kwargs)?;
    if let Some(r) = redis {
        let n: i64 = r.lpush(&body).await.with_context(|| format!("redis LPUSH {}", r.queue_key()))?;
        eprintln!("  pushed {} (cache_key={}, queue_len={})", name, cache_key, n);
    } else {
        anyhow::bail!("submit_one_redis: no redis connection available");
    }
    Ok(cache_key)
}

pub async fn submit_one_ssh(
    ssh: &mut SshClient,
    game: &str,
    name: &str,
    kwargs: serde_json::Value,
) -> Result<String> {
    let cache_key = format!("{game}/audio/{name}");
    let body = build_envelope(game, name, kwargs)?;
    let cmd = format!("redis-cli -x LPUSH {QUEUE}");
    let (_out, err, code) = ssh.exec_stdin(&cmd, body.as_bytes()).await
        .with_context(|| format!("ssh LPUSH {QUEUE}"))?;
    if code != 0 {
        anyhow::bail!("redis-cli LPUSH exit={code} stderr={err}");
    }
    eprintln!("  pushed {} (cache_key={})", name, cache_key);
    Ok(cache_key)
}

fn build_envelope(game: &str, name: &str, kwargs: serde_json::Value) -> Result<String> {
    let serde_json::Value::Object(mut kw) = kwargs else {
        anyhow::bail!("kwargs must be a JSON object");
    };
    kw.insert("format".into(), serde_json::Value::String("mp3".into()));
    kw.insert("cache_key".into(), serde_json::Value::String(format!("{game}/audio/{name}")));
    let envelope = serde_json::json!({
        "id": format!("{}-aud-{}-{}", game, name, std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()),
        "task": "audio.gen",
        "kwargs": kw,
        "retries": 0,
        "max_retries": 1,
        "priority": "normal",
    });
    serde_json::to_string(&envelope).context("serialize envelope")
}

/// Build kwargs from a JobSpec for MiniMax.
pub fn build_kwargs(jf: &JobsFile, job: &crate::jobs::JobSpec) -> Result<serde_json::Value> {
    let mut kw = serde_json::Map::new();
    kw.insert("sample_rate".into(), serde_json::json!(job.sample_rate));
    kw.insert("channels".into(), serde_json::json!(job.channels));
    if job.bitrate > 0 {
        kw.insert("bitrate".into(), serde_json::json!(job.bitrate));
    }
    match job.kind {
        JobKind::Music => {
            kw.insert("mode".into(), serde_json::json!("music"));
            let base = jf.style.as_deref().unwrap_or("");
            let prompt = format!("{base}; {}", job.prompt.as_deref().unwrap_or(""));
            kw.insert("prompt".into(), serde_json::json!(prompt.trim_matches(';').trim()));
        }
        JobKind::Voice => {
            kw.insert("mode".into(), serde_json::json!("voice"));
            kw.insert("text".into(), serde_json::json!(job.text.clone().unwrap_or_default()));
            kw.insert("voice_id".into(), serde_json::json!(
                job.voice_id.clone().or_else(|| jf.voice_id.clone()).unwrap_or_default()
            ));
            if let Some(sp) = job.speed {
                kw.insert("speed".into(), serde_json::json!(sp));
            }
        }
        JobKind::Procedural => {
            // Procedural jobs are NOT submitted to VPS — handled locally.
            anyhow::bail!("build_kwargs called on procedural job: {}", job.name);
        }
    }
    Ok(serde_json::Value::Object(kw))
}

/// Submit all AI jobs (music + voice) from a JobsFile. Procedural jobs are skipped.
/// Pass `redis` if `AUDIOGEN_REDIS_URL` is set; else fall back to `ssh`.
pub async fn submit(
    jf: &JobsFile,
    redis: Option<&RedisQueue>,
    mut ssh: Option<&mut SshClient>,
    only: Option<&[String]>,
) -> Result<Vec<String>> {
    let output_dir = PathBuf::from(&jf.output_dir);
    std::fs::create_dir_all(&output_dir)?;
    let mut pending = load_pending(&output_dir, &jf.game)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    let mut cache_keys = Vec::new();
    for job in &jf.jobs {
        if let Some(filter) = only {
            if !filter.contains(&job.name) { continue; }
        }
        if matches!(job.kind, JobKind::Procedural) {
            eprintln!("  skip procedural: {} (run `audiogen sfx` instead)", job.name);
            continue;
        }
        let kwargs = build_kwargs(jf, job)?;
        let ck = if let Some(r) = redis {
            submit_one_redis(Some(r), &jf.game, &job.name, kwargs).await?
        } else if let Some(s) = ssh.as_mut() {
            submit_one_ssh(s, &jf.game, &job.name, kwargs).await?
        } else {
            anyhow::bail!("submit: neither redis nor ssh connection available");
        };
        pending.jobs.insert(ck.clone(), PendingJob {
            cache_key: ck.clone(),
            kind: job.kind.clone(),
            format: job.format.clone(),
            submitted_at: now,
        });
        cache_keys.push(ck);
    }
    pending.last_submit_at = Some(now);
    save_pending(&output_dir, &pending)?;
    Ok(cache_keys)
}

/// Result envelope returned by rustyme via BRPOP.
#[derive(Debug, Clone, Deserialize)]
struct ResultEnvelope {
    result: Option<ResultPayload>,
}

#[derive(Debug, Clone, Deserialize)]
struct ResultPayload {
    cache_key: String,
    audio_b64: String,
    #[serde(default)]
    duration_ms: Option<u64>,
}

/// Collect results from VPS until pending is empty or `max_wait_secs` elapses.
/// Decodes base64 → writes to `<output_dir>/<name>.<format>` and clears from pending.
/// `redis` preferred, `ssh` fallback.
pub async fn collect(
    jf: &JobsFile,
    redis: Option<&RedisQueue>,
    mut ssh: Option<&mut SshClient>,
    max_wait_secs: u64,
) -> Result<usize> {
    let output_dir = PathBuf::from(&jf.output_dir);
    let mut pending = load_pending(&output_dir, &jf.game)?;
    if pending.jobs.is_empty() {
        eprintln!("nothing pending");
        return Ok(0);
    }
    let want: HashMap<String, String> = pending.jobs.values()
        .map(|j| (j.cache_key.clone(), j.format.clone()))
        .collect();
    let pending_keys: Vec<String> = pending.jobs.keys().cloned().collect();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(max_wait_secs);
    let mut got = 0usize;
    while !pending.jobs.is_empty() && std::time::Instant::now() < deadline {
        let pop_result: Option<(String, String)> = if let Some(r) = redis {
            r.brpop(30).await?
        } else if let Some(ssh_ref) = ssh.as_mut() {
            let cmd = format!("redis-cli BRPOP {RESULT} 30");
            let (out, _err, code) = ssh_ref.exec(&cmd).await?;
            if code != 0 || out.trim().is_empty() { continue; }
            // BRPOP 输出格式: "<key>\n<value>\n"
            let val = match out.find('\n') {
                Some(i) => out[i + 1..].trim(),
                None => out.trim(),
            };
            Some((RESULT.to_string(), val.to_string()))
        } else {
            anyhow::bail!("collect: neither redis nor ssh connection available");
        };
        let Some((_key, val)) = pop_result else { continue };
        let env: ResultEnvelope = match serde_json::from_str(&val) {
            Ok(v) => v,
            Err(e) => { eprintln!("  skip non-JSON result: {e}"); continue; }
        };
        let Some(payload) = env.result else {
            eprintln!("  skip result without payload");
            continue;
        };
        if !want.contains_key(&payload.cache_key) {
            eprintln!("  skip non-matching cache_key={}", payload.cache_key);
            continue;
        }
        let fmt = want[&payload.cache_key].clone();
        let name = payload.cache_key.split('/').last().unwrap_or(&payload.cache_key);
        let bytes = base64::engine::general_purpose::STANDARD.decode(&payload.audio_b64)
            .context("base64 decode audio_b64")?;
        let out_path = output_dir.join(format!("{name}.{fmt}"));
        std::fs::write(&out_path, &bytes)?;
        pending.jobs.remove(&payload.cache_key);
        got += 1;
        eprintln!("  [ok] {name}.{fmt}  {} bytes  dur={}ms",
                  bytes.len(),
                  payload.duration_ms.map(|d| d.to_string()).unwrap_or_else(|| "?".into()));
    }
    save_pending(&output_dir, &pending)?;
    eprintln!("== collected {got} / {} pending ==", pending_keys.len());
    Ok(got)
}

/// Run procedural jobs locally (synthesize → encode → write).
pub fn run_procedural(jf: &JobsFile, only: Option<&[String]>) -> Result<usize> {
    let output_dir = PathBuf::from(&jf.output_dir);
    std::fs::create_dir_all(&output_dir)?;
    let mut n = 0;
    for job in &jf.jobs {
        if let Some(filter) = only {
            if !filter.contains(&job.name) { continue; }
        }
        if !matches!(job.kind, JobKind::Procedural) { continue; }
        let synth_name = job.synth.as_deref().unwrap_or("");
        let dur = job.duration_secs.unwrap_or(0.5);
        let pcm = synth::synthesize(synth_name, dur, job.sample_rate, job.channels)?;
        let codec = codec_for_ext(&job.format)?;
        let cfg = EncodeCfg::mp3(job.sample_rate, job.channels, job.bitrate.max(32_000));
        let bytes = codec.encode(&pcm, &cfg)?;
        let out_path = output_dir.join(format!("{}.{}", job.name, job.format));
        std::fs::write(&out_path, &bytes)?;
        eprintln!("  [proc] {}  {} bytes → {}", job.name, bytes.len(), out_path.display());
        n += 1;
    }
    Ok(n)
}

/// Copy generated audio into the Godot project tree for import.
pub fn install(jf: &JobsFile) -> Result<usize> {
    let Some(install_dir) = jf.install_dir.as_ref() else {
        anyhow::bail!("jobs file has no `install_dir`; cannot install");
    };
    let src = PathBuf::from(&jf.output_dir);
    let dst = PathBuf::from(install_dir);
    std::fs::create_dir_all(&dst)?;
    let mut n = 0;
    for entry in std::fs::read_dir(&src)? {
        let entry = entry?;
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()).map_or(true, |e| !["mp3","wav","ogg"].contains(&e)) {
            continue;
        }
        let name = p.file_name().unwrap();
        std::fs::copy(&p, dst.join(name))?;
        n += 1;
    }
    eprintln!("copied {n} audio files → {}", dst.display());
    Ok(n)
}