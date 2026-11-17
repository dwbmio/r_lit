//! phone_haul — host-side orchestrator (macOS).
//!
//! Subcommands:
//!   scan                     push agent, run device scan, save manifest.json, print summary
//!   pull --dest DIR [-j N]   dedup vs local redb, parallel stream files, verify, place by yyyy/MM

use phone_haul::{FileEntry, STREAM_MAGIC};
mod ios;
mod keys;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};

const AGENT_LOCAL: &str = "out/aarch64-linux-android/phone_haul_agent"; // prebuilt Android binary
const AGENT_REMOTE: &str = "/data/local/tmp/phone_haul_agent";
const DB_FILE: &str = "phone_haul.redb";
const TABLE: redb::TableDefinition<&str, &str> = redb::TableDefinition::new("b3_to_path");

#[derive(Parser)]
#[command(name = "phone_haul", about = "Parallel dedup-aware media migration over adb")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
    /// adb executable (default: $ADB or "adb" on PATH)
    #[arg(long, global = true)]
    adb: Option<String>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan device media dirs and save manifest.json
    Scan {
        /// extra roots on device (defaults: DCIM/Pictures/Movies/Screenshots)
        #[arg(long)]
        root: Vec<String>,
        /// where to save manifest.json
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// Pull files: dedup, verify, place under dest/yyyy/MM/
    Pull {
        #[arg(long)]
        dest: PathBuf,
        /// manifest.json produced by `scan`
        #[arg(long, default_value = "manifest.json")]
        manifest: PathBuf,
        /// parallel adb streams
        #[arg(long, short = 'j', default_value_t = 8)]
        jobs: usize,
        /// only pull images (skip videos/other media)
        #[arg(long)]
        images_only: bool,
    },
    /// [iOS] pull Photos.sqlite over AFC and dump asset keys (iphone_keys.json)
    IosScan {
        #[arg(long, default_value = ".")]
        out: PathBuf,
    },
    /// [iOS] diff a local archive vs the iPhone photo library; hardlink missing into --out
    Diff {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// [iOS] regroup a flat staging dir into dest/yyyy/MM album tree for Finder photo sync
    PushIos {
        /// flat dir of files to push (e.g. produced by `diff --out`)
        #[arg(long)]
        from: PathBuf,
        /// destination sync folder (pick this folder in Finder -> iPhone -> Photos)
        #[arg(long)]
        dest: PathBuf,
    },
    /// collect low-confidence files (no EXIF/mvhd key, e.g. PNG screenshots) for manual review
    CollectLowconf {
        #[arg(long)]
        archive: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// [iOS] pull every screenshot from the iPhone photo library into --out for review
    PullScreenshots {
        #[arg(long)]
        out: PathBuf,
    },
}

fn adb_path(cli_adb: &Option<String>) -> String {
    cli_adb
        .clone()
        .or_else(|| std::env::var("ADB").ok())
        .unwrap_or_else(|| "adb".into())
}

fn ensure_agent(adb: &str) -> Result<()> {
    if !Path::new(AGENT_LOCAL).exists() {
        bail!(
            "missing prebuilt agent at {AGENT_LOCAL}; build with: \
             cargo zigbuild --release --target aarch64-linux-android --bin phone_haul_agent \
             && mkdir -p out/aarch64-linux-android \
             && cp target/aarch64-linux-android/release/phone_haul_agent out/aarch64-linux-android/"
        );
    }
    run_ok(Command::new(adb)
        .args(["push", AGENT_LOCAL, AGENT_REMOTE]))?;
    run_ok(Command::new(adb)
        .args(["shell", "chmod", "755", AGENT_REMOTE]))?;
    Ok(())
}

fn run_ok(cmd: &mut Command) -> Result<()> {
    let st = cmd.status().with_context(|| format!("spawn {cmd:?}"))?;
    if !st.success() {
        bail!("{cmd:?} exited {st}");
    }
    Ok(())
}

fn cmd_scan(adb: &str, roots: &[String], out: &Path) -> Result<()> {
    ensure_agent(adb)?;
    let mut args = vec!["exec-out".into(), AGENT_REMOTE.into(), "scan".into()];
    args.extend(roots.iter().cloned());
    let output = Command::new(adb)
        .args(&args)
        .output()
        .context("run agent scan")?;
    if !output.status.success() {
        bail!("agent scan failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    let entries: Vec<FileEntry> =
        serde_json::from_slice(&output.stdout).context("parse manifest")?;
    fs::create_dir_all(out)?;
    fs::write(out.join("manifest.json"), &output.stdout)?;

    let mut by_kind: HashMap<String, (u64, u64)> = HashMap::new();
    let mut seen: HashMap<(u64, &str), ()> = HashMap::new();
    let mut dup_bytes = 0u64;
    let mut dup_cnt = 0u64;
    for e in &entries {
        let key = (e.size, e.quick.as_str());
        let slot = by_kind.entry(format!("{:?}", e.kind)).or_default();
        slot.0 += 1;
        slot.1 += e.size;
        if seen.insert(key, ()).is_some() {
            dup_cnt += 1;
            dup_bytes += e.size;
        }
    }
    let total: u64 = entries.iter().map(|e| e.size).sum();
    println!("files: {}  total: {:.2} GiB", entries.len(), total as f64 / (1 << 30) as f64);
    println!("on-device dups: {} files / {:.2} GiB (will transfer once)",
        dup_cnt, dup_bytes as f64 / (1 << 30) as f64);
    let mut kinds: Vec<_> = by_kind.into_iter().collect();
    kinds.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
    for (k, (n, b)) in kinds {
        println!("  {k:6} {n:6} files  {:8.2} GiB", b as f64 / (1 << 30) as f64);
    }
    println!("manifest saved to {}", out.join("manifest.json").display());
    Ok(())
}

struct StreamReader {
    #[allow(dead_code)]
    child: Child,
    stdout: BufReader<ChildStdout>,
    adb: String,
    remote_list: String,
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        let _ = Command::new(&self.adb)
            .args(["shell", "rm", "-f", &self.remote_list])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

impl StreamReader {
    fn spawn(adb: &str, batch: &[FileEntry], idx: usize) -> Result<Self> {
        // adb exec-out does not forward stdin on many adb versions ->
        // push the batch list as a file and pass its path as argv.
        let remote_list = format!("/data/local/tmp/phone_haul_batch_{}_{idx}.txt", std::process::id());
        let local_list = std::env::temp_dir().join(format!("phone_haul_batch_{}_{idx}.txt", std::process::id()));
        {
            let mut f = File::create(&local_list)?;
            for e in batch {
                f.write_all(e.path.as_bytes())?;
                f.write_all(b"\n")?;
            }
        }
        run_ok(Command::new(adb).args([
            "push",
            &local_list.to_string_lossy(),
            &remote_list,
        ]))?;
        let _ = fs::remove_file(&local_list);
        let mut child = Command::new(adb)
            .args(["exec-out", AGENT_REMOTE, "stream", &remote_list])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .context("spawn adb exec-out")?;
        let mut stdout = BufReader::with_capacity(1 << 20, child.stdout.take().unwrap());
        let mut magic = [0u8; 4];
        stdout.read_exact(&mut magic)?;
        if u32::from_le_bytes(magic) != STREAM_MAGIC {
            bail!("bad stream magic");
        }
        Ok(Self {
            child,
            stdout,
            adb: adb.to_string(),
            remote_list,
        })
    }

    /// Read next record: hash while writing to staging file.
    /// Returns (device path, size, full blake3 hex, staging tmp file).
    fn next_file(&mut self, staging: &Path) -> Result<Option<(String, u64, String, PathBuf)>> {
        let mut len = [0u8; 4];
        self.stdout.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len == 0 {
            return Ok(None);
        }
        let mut path = vec![0u8; len];
        self.stdout.read_exact(&mut path)?;
        let mut sizeb = [0u8; 8];
        self.stdout.read_exact(&mut sizeb)?;
        let size = u64::from_le_bytes(sizeb);

        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = staging.join(format!(".part-{}-{id}", std::process::id()));
        let mut out = File::create(&tmp)?;
        out.set_len(size)?; // ftruncate preallocate
        let mut hasher = blake3::Hasher::new();
        let mut remaining = size;
        let mut buf = vec![0u8; 1 << 20];
        while remaining > 0 {
            let want = remaining.min(buf.len() as u64) as usize;
            self.stdout.read_exact(&mut buf[..want])?;
            hasher.update(&buf[..want]);
            out.write_all(&buf[..want])?;
            remaining -= want as u64;
        }
        drop(out);
        Ok(Some((
            String::from_utf8_lossy(&path).into_owned(),
            size,
            hasher.finalize().to_hex().to_string(),
            tmp,
        )))
    }
}

fn yyyy_mm(mtime: i64) -> String {
    // civil-from-days (no chrono dep): days since epoch -> (y, m)
    let days = mtime.div_euclid(86400);
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}/{m:02}")
}

fn cmd_pull(adb: &str, dest: &Path, manifest: &Path, jobs: usize, images_only: bool) -> Result<()> {
    ensure_agent(adb)?;
    let entries: Vec<FileEntry> =
        serde_json::from_slice(&fs::read(manifest)?).context("read manifest")?;
    fs::create_dir_all(dest)?;
    let staging = dest.join(".staging");
    fs::create_dir_all(&staging)?;

    let db = redb::Database::create(dest.join(DB_FILE))?;
    {
        let tx = db.begin_write()?;
        tx.open_table(TABLE)?;
        tx.commit()?;
    }

    // on-device dedup: first of each (size, quick) group wins
    let mut seen = HashMap::new();
    let mut todo: Vec<FileEntry> = Vec::new();
    for e in entries {
        if images_only && !e.kind.is_image() {
            continue;
        }
        if seen.insert((e.size, e.quick.clone()), ()).is_none() {
            todo.push(e);
        }
    }
    let total: u64 = todo.iter().map(|e| e.size).sum();
    println!("to transfer: {} files / {:.2} GiB ({} jobs)",
        todo.len(), total as f64 / (1 << 30) as f64, jobs);

    // round-robin into per-worker batches
    let mut batches: Vec<Vec<FileEntry>> = vec![Vec::new(); jobs.max(1)];
    let njobs = batches.len();
    for (i, e) in todo.into_iter().enumerate() {
        batches[i % njobs].push(e);
    }

    let adb = adb.to_string();
    let dest = dest.to_path_buf();
    let db = &db;
    std::thread::scope(|s| -> Result<()> {
        let mut handles = Vec::new();
        for (idx, batch) in batches.into_iter().enumerate().filter(|(_, b)| !b.is_empty()) {
            let (adb, dest, staging, db) = (&adb, &dest, &staging, db);
            handles.push(s.spawn(move || -> Result<(u64, u64)> {
                let mut sr = StreamReader::spawn(adb, &batch, idx)?;
                let mut ok = 0u64;
                let mut skipped = 0u64;
                while let Some((dev_path, _size, b3, tmp)) = sr.next_file(staging)? {
                    // dedup against local db (authoritative full hash)
                    let exists = {
                        let tx = db.begin_read()?;
                        let t = tx.open_table(TABLE)?;
                        t.get(b3.as_str())?.is_some()
                    };
                    if exists {
                        let _ = fs::remove_file(&tmp);
                        skipped += 1;
                        continue;
                    }
                    let mtime = batch
                        .iter()
                        .find(|b| b.path == dev_path)
                        .map(|b| b.mtime)
                        .unwrap_or(0);
                    let name = dev_path.rsplit('/').next().unwrap_or("file");
                    let dir = dest.join(yyyy_mm(mtime));
                    fs::create_dir_all(&dir)?;
                    let target = unique_path(&dir, name);
                    fs::rename(&tmp, &target)?;
                    let tx = db.begin_write()?;
                    {
                        let mut t = tx.open_table(TABLE)?;
                        t.insert(b3.as_str(), target.to_string_lossy().as_ref())?;
                    }
                    tx.commit()?;
                    ok += 1;
                }
                Ok((ok, skipped))
            }));
        }
        let (mut ok, mut skipped) = (0u64, 0u64);
        for h in handles {
            let (a, b) = h.join().map_err(|_| anyhow::anyhow!("worker panicked"))??;
            ok += a;
            skipped += b;
        }
        println!("done: {ok} transferred, {skipped} skipped (already in local db)");
        Ok(())
    })
}

fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    if !p.exists() {
        return p;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.to_string(), String::new()),
    };
    for i in 1.. {
        let p = dir.join(format!("{stem}_{i}{ext}"));
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

fn tokio_rt() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?)
}

fn cmd_ios_scan(out: &Path) -> Result<()> {
    let rt = tokio_rt()?;
    rt.block_on(async {
        let mut afc = ios::connect_afc().await?;
        let db = ios::pull_photos_db(&mut afc, &out.join("ios_db")).await?;
        let ks = keys::iphone_keys(&db)?;
        std::fs::create_dir_all(out)?;
        std::fs::write(out.join("iphone_keys.json"), serde_json::to_vec(&ks)?)?;
        println!("iphone assets: {}", ks.len());
        Ok(())
    })
}

fn cmd_diff(archive: &Path, out: &Path) -> Result<()> {
    let rt = tokio_rt()?;
    let ik = rt.block_on(async {
        let mut afc = ios::connect_afc().await?;
        let dir = std::env::temp_dir().join(format!("phone_haul_ios_{}", std::process::id()));
        let db = ios::pull_photos_db(&mut afc, &dir).await?;
        keys::iphone_keys(&db)
    })?;
    println!("iphone assets: {}", ik.len());

    let (lk, low) = keys::local_keys(archive);
    println!("local archive keyed: {}  (low-confidence skipped: {})", lk.len(), low.len());

    let iset: std::collections::HashSet<(i64, u32, u32)> =
        ik.iter().map(|e| (e.t, e.w, e.h)).collect();
    let itset: std::collections::HashSet<i64> = ik.iter().map(|e| e.t).collect();
    let mut missing = Vec::new();
    for e in &lk {
        let hit = iset.contains(&(e.t, e.w, e.h)) || (e.w == 0 && itset.contains(&e.t));
        if !hit {
            missing.push(e.path.clone());
        }
    }
    std::fs::create_dir_all(out)?;
    for p in &missing {
        let name = std::path::Path::new(p).file_name().unwrap();
        let dst = out.join(name);
        if !dst.exists() {
            std::fs::hard_link(p, &dst).or_else(|_| std::fs::copy(p, &dst).map(|_| ()))?;
        }
    }
    let report = serde_json::json!({"missing": missing, "low_confidence": low});
    std::fs::write(out.join("missing_report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("missing on iphone: {} -> linked into {}", missing.len(), out.display());
    Ok(())
}

fn link_into(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        return Ok(());
    }
    if let Some(d) = dst.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::hard_link(src, dst).or_else(|_| std::fs::copy(src, dst).map(|_| ()))?;
    Ok(())
}

fn cmd_push_ios(from: &Path, dest: &Path) -> Result<()> {
    let (keyed, low) = keys::local_keys(from);
    for k in &keyed {
        let name = Path::new(&k.path).file_name().unwrap();
        link_into(Path::new(&k.path), &dest.join(yyyy_mm(k.t)).join(name))?;
    }
    // no reliable shot time -> park under _unknown so user can triage
    for p in &low {
        let src = Path::new(p);
        let name = src.file_name().unwrap();
        link_into(src, &dest.join("_unknown").join(name))?;
    }
    println!(
        "staged {} keyed (yyyy/MM albums) + {} unknown -> {}",
        keyed.len(),
        low.len(),
        dest.display()
    );
    println!("next: Finder -> iPhone -> 照片 -> 同步自文件夹 [{}]", dest.display());
    Ok(())
}

fn cmd_collect_lowconf(archive: &Path, out: &Path) -> Result<()> {
    let (_keyed, low) = keys::local_keys(archive);
    let prefix = archive.canonicalize()?;
    for p in &low {
        let src = Path::new(p);
        let rel = src
            .canonicalize()
            .ok()
            .and_then(|c| c.strip_prefix(&prefix).ok().map(|r| r.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from(src.file_name().unwrap()));
        link_into(src, &out.join(rel))?;
    }
    println!("collected {} low-confidence files -> {}", low.len(), out.display());
    Ok(())
}

fn cmd_pull_screenshots(out: &Path) -> Result<()> {
    let rt = tokio_rt()?;
    rt.block_on(async {
        let mut afc = ios::connect_afc().await?;
        let dir = std::env::temp_dir().join(format!("phone_haul_ios_{}", std::process::id()));
        let db = ios::pull_photos_db(&mut afc, &dir).await?;
        let paths = keys::iphone_screenshot_paths(&db)?;
        std::fs::create_dir_all(out)?;
        let total = paths.len();
        let (mut ok, mut failed) = (0u32, Vec::new());
        for (i, rel) in paths.iter().enumerate() {
            let dst = out.join(rel.rsplit('/').next().unwrap_or(rel));
            if dst.exists() {
                ok += 1;
                continue;
            }
            match ios::pull_file(&mut afc, &format!("/{rel}"), &dst).await {
                Ok(_) => ok += 1,
                Err(_) => failed.push(rel.clone()), // iCloud 云端原件未下载到本机的会失败
            }
            if (i + 1) % 200 == 0 {
                println!("  {}/{total} ...", i + 1);
            }
        }
        println!("screenshots: {ok}/{total} pulled -> {}", out.display());
        if !failed.is_empty() {
            println!("  {} cloud-only (原件不在本机, 跳过)", failed.len());
        }
        Ok(())
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let adb = adb_path(&cli.adb);
    match &cli.cmd {
        Cmd::Scan { root, out } => cmd_scan(&adb, root, out),
        Cmd::Pull {
            dest,
            manifest,
            jobs,
            images_only,
        } => cmd_pull(&adb, dest, manifest, *jobs, *images_only),
        Cmd::IosScan { out } => cmd_ios_scan(out),
        Cmd::Diff { archive, out } => cmd_diff(archive, out),
        Cmd::PushIos { from, dest } => cmd_push_ios(from, dest),
        Cmd::CollectLowconf { archive, out } => cmd_collect_lowconf(archive, out),
        Cmd::PullScreenshots { out } => cmd_pull_screenshots(out),
    }
}