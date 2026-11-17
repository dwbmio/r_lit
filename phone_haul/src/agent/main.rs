//! phone_haul_agent — runs ON the Android device (pushed to /data/local/tmp).
//! Short-lived: scan or stream, then exit. std + walkdir + blake3 only.
//!
//! Usage:
//!   phone_haul_agent scan [root ...]        # manifest JSON -> stdout
//!   phone_haul_agent stream <listfile>      # newline paths from listfile -> framed stream to stdout
//!   phone_haul_agent stream                 # fallback: paths from stdin (adb exec-out may not forward stdin)

use phone_haul::{classify, quick_hash, FileEntry, MediaKind, SNIFF_LEN, STREAM_MAGIC};
use std::fs::File;
use std::io::{self, Read, Write};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const DEFAULT_ROOTS: &[&str] = &[
    "/storage/emulated/0/DCIM",
    "/storage/emulated/0/Pictures",
    "/storage/emulated/0/Movies",
    "/storage/emulated/0/Screenshots",
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let code = match args.get(1).map(String::as_str) {
        Some("scan") => scan(&args[2..]),
        Some("stream") => stream(args.get(2).map(String::as_str)),
        _ => {
            eprintln!("usage: phone_haul_agent scan [root...] | stream");
            2
        }
    };
    std::process::exit(code);
}

fn scan(roots: &[String]) -> i32 {
    let roots: Vec<String> = if roots.is_empty() {
        DEFAULT_ROOTS.iter().map(|s| s.to_string()).collect()
    } else {
        roots.to_vec()
    };
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let _ = out.write_all(b"[");
    let mut first = true;
    for root in &roots {
        for e in WalkDir::new(root).follow_links(false).into_iter().flatten() {
            if !e.file_type().is_file() {
                continue;
            }
            let path = e.path().to_string_lossy().to_string();
            // skip system thumbnail caches / trash — junk for migration
            if path.contains("/.thumbnails/") || path.contains("/.trash") {
                continue;
            }
            let Ok(mut f) = File::open(e.path()) else { continue };
            let mut head = [0u8; SNIFF_LEN];
            let Ok(n) = f.read(&mut head) else { continue };
            let kind = classify(&head[..n]);
            if kind == MediaKind::Other {
                continue; // only media files
            }
            let Ok(meta) = e.metadata() else { continue };
            let size = meta.len();
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let Ok(quick) = quick_hash(&mut f, size) else { continue };
            let entry = FileEntry {
                path,
                size,
                mtime,
                kind,
                quick,
            };
            if !first {
                let _ = out.write_all(b",");
            }
            first = false;
            if serde_json::to_writer(&mut out, &entry).is_err() {
                return 1;
            }
        }
    }
    let _ = out.write_all(b"]");
    let _ = out.flush();
    0
}

fn stream(listfile: Option<&str>) -> i32 {
    let list: String = match listfile {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(_) => return 1,
        },
        None => {
            let mut s = String::new();
            if io::stdin().lock().read_to_string(&mut s).is_err() {
                return 1;
            }
            s
        }
    };
    let stdout = io::stdout();
    let mut out = io::BufWriter::with_capacity(1 << 20, stdout.lock());
    if out.write_all(&STREAM_MAGIC.to_le_bytes()).is_err() {
        return 1;
    }
    let mut buf = vec![0u8; 1 << 20];
    for path in list.lines() {
        let path = path.trim();
        if path.is_empty() {
            continue;
        }
        let Ok(mut f) = File::open(&path) else {
            continue; // host reconciles missing files against manifest
        };
        let size = f.metadata().map(|m| m.len()).unwrap_or(0);
        let pb = path.as_bytes();
        let frame = [
            &(pb.len() as u32).to_le_bytes(),
            pb,
            &size.to_le_bytes(),
        ]
        .concat();
        if out.write_all(&frame).is_err() {
            return 1;
        }
        loop {
            match f.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out.write_all(&buf[..n]).is_err() {
                        return 1;
                    }
                }
                Err(_) => break,
            }
        }
    }
    let _ = out.write_all(&0u32.to_le_bytes()); // terminator
    let _ = out.flush();
    0
}
