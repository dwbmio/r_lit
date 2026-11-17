//! Media identity keys: (shot_time_local_unix, width, height).
//! iPhone side  : Photos.sqlite ZASSET (rusqlite)
//! Local archive: JPEG EXIF (kamadak-exif) / MP4·MOV mvhd box / others -> low-confidence
//!
//! 所有键统一为绝对 unix 秒：EXIF 本地墙钟 N 与绝对 A 的关系是 A = N - offset，
//! 因此本地 JPEG 键 = N - offset，MP4 mvhd / ZDATECREATED 本身即绝对 UTC，不加不减。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const CF_EPOCH: i64 = 978_307_200; // 2001-01-01 UTC
const MP4_EPOCH: i64 = 2_082_844_800; // 1904-01-01 UTC

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaKey {
    pub path: String,
    pub t: i64,
    pub w: u32,
    pub h: u32,
}

pub fn local_offset() -> i64 {
    unsafe {
        let t = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&t, &mut tm);
        tm.tm_gmtoff
    }
}

/// days-from-civil (Howard Hinnant) -> days since 1970-01-01
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// "YYYY:MM:DD HH:MM:SS" (EXIF local wall clock) -> naive wall seconds N
fn exif_datetime_to_key(s: &str) -> Option<i64> {
    let mut n = [0i64; 6];
    let mut idx = 0;
    for part in s.split(|c: char| !c.is_ascii_digit()) {
        if part.is_empty() {
            continue;
        }
        if idx >= 6 {
            return None;
        }
        n[idx] = part.parse().ok()?;
        idx += 1;
    }
    if idx < 6 {
        return None;
    }
    Some(days_from_civil(n[0], n[1], n[2]) * 86400 + n[3] * 3600 + n[4] * 60 + n[5])
}

fn jpeg_key(path: &Path, offset: i64) -> Option<MediaKey> {
    let f = std::fs::File::open(path).ok()?;
    let ex = exif::Reader::new()
        .read_from_container(&mut BufReader::new(f))
        .ok()?;
    let dto = ex
        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
        .or_else(|| ex.get_field(exif::Tag::DateTime, exif::In::PRIMARY))?;
    let t = exif_datetime_to_key(&dto.display_value().to_string())? - offset;
    let dim = |tag: exif::Tag| {
        ex.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .unwrap_or(0)
    };
    Some(MediaKey {
        path: path.to_string_lossy().into_owned(),
        t,
        w: dim(exif::Tag::PixelXDimension),
        h: dim(exif::Tag::PixelYDimension),
    })
}

fn filename_epoch(path: &Path) -> Option<i64> {
    let name = path.file_name()?.to_string_lossy();
    let digits: String = name.chars().filter(|c| c.is_ascii_digit()).collect();
    // mmexport1725646732170.mp4 -> 13 位 epoch ms (微信导出, 绝对时间)
    if name.starts_with("mmexport") && digits.len() >= 13 {
        return digits[..13].parse::<i64>().ok().map(|ms| ms / 1000);
    }
    // lv_7208059650615315716_20241115184113.mp4 -> 尾部 yyyyMMddhhmmss (抖音, 本地墙钟)
    if digits.len() >= 14 {
        let tail = &digits[digits.len() - 14..];
        let n: Vec<i64> = (0..6)
            .map(|i| match i {
                0 => tail[0..4].parse().ok(),
                1 => tail[4..6].parse().ok(),
                2 => tail[6..8].parse().ok(),
                3 => tail[8..10].parse().ok(),
                4 => tail[10..12].parse().ok(),
                _ => tail[12..14].parse().ok(),
            })
            .collect::<Option<_>>()?;
        // 校验合法性, 纯数字文件名(非日期)不应被误解析
        let valid = (1990..=2100).contains(&n[0])
            && (1..=12).contains(&n[1])
            && (1..=31).contains(&n[2])
            && n[3] < 24
            && n[4] < 60
            && n[5] < 60;
        if !valid {
            return None;
        }
        let wall = days_from_civil(n[0], n[1], n[2]) * 86400 + n[3] * 3600 + n[4] * 60 + n[5];
        return Some(wall - local_offset());
    }
    None
}

fn mp4_key(path: &Path, offset: i64) -> Option<MediaKey> {
    let mut f = std::fs::File::open(path).ok()?;
    let end = f.metadata().ok()?.len();
    fn walk(f: &mut std::fs::File, end: u64, depth: u8) -> Option<i64> {
        if depth > 6 {
            return None;
        }
        while f.stream_position().ok()? + 8 <= end {
            let pos = f.stream_position().ok()?;
            let mut hdr = [0u8; 8];
            f.read_exact(&mut hdr).ok()?;
            let mut size = u32::from_be_bytes(hdr[..4].try_into().ok()?) as u64;
            let typ = &hdr[4..8];
            let mut header = 8u64;
            if size == 1 {
                let mut b = [0u8; 8];
                f.read_exact(&mut b).ok()?;
                size = u64::from_be_bytes(b);
                header = 16;
            }
            if size < header || pos + size > end + 8 {
                return None;
            }
            if typ == b"mvhd" {
                let mut ver = [0u8; 4];
                f.read_exact(&mut ver).ok()?;
                let ct = if ver[0] == 1 {
                    let mut b = [0u8; 8];
                    f.read_exact(&mut b).ok()?;
                    u64::from_be_bytes(b)
                } else {
                    let mut b = [0u8; 4];
                    f.read_exact(&mut b).ok()?;
                    u32::from_be_bytes(b) as u64
                };
                return Some(ct as i64 - MP4_EPOCH);
            }
            if matches!(typ, b"moov" | b"trak" | b"mdia" | b"minf") {
                if let Some(r) = walk(f, pos + size, depth + 1) {
                    return Some(r);
                }
            }
            f.seek(SeekFrom::Start(pos + size)).ok()?;
        }
        None
    }
    let utc = walk(&mut f, end, 0)
        .filter(|&utc| utc > 0) // mvhd creation_time=0 (1904) 视为无效, 走文件名兜底
        .or_else(|| filename_epoch(path))?;
    let _ = offset; // mvhd creation_time / filename epoch are absolute already
    Some(MediaKey {
        path: path.to_string_lossy().into_owned(),
        t: utc,
        w: 0,
        h: 0,
    })
}

/// Walk a phone_haul archive; returns (keyed, low_confidence_paths).
pub fn local_keys(archive: &Path) -> (Vec<MediaKey>, Vec<String>) {
    let offset = local_offset();
    let mut keys = Vec::new();
    let mut low = Vec::new();
    for e in walkdir::WalkDir::new(archive)
        .follow_links(true) // to_iphone staging uses hardlinks; archive is real files
        .into_iter()
        .flatten()
    {
        if !e.file_type().is_file() || e.path().to_string_lossy().contains("/.staging/") {
            continue;
        }
        let p = e.path();
        let ext = p
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        const MEDIA: &[&str] = &[
            "jpg", "jpeg", "png", "gif", "webp", "heic", "avif", "dng", "mp4", "mov", "webm",
        ];
        if !MEDIA.contains(&ext.as_str()) {
            continue; // skip manifests / reports / db sidecars inside the tree
        }
        let k = match ext.as_str() {
            "jpg" | "jpeg" => jpeg_key(p, offset),
            "mp4" | "mov" => mp4_key(p, offset),
            _ => None,
        };
        match k {
            Some(k) => keys.push(k),
            None => low.push(p.to_string_lossy().into_owned()),
        }
    }
    (keys, low)
}

/// Paths (relative to AFC root) of all screenshots: ZKIND=0(image), ZKINDSUBTYPE=2(screenshot).
pub fn iphone_screenshot_paths(db: &Path) -> Result<Vec<String>> {
    let con = rusqlite::Connection::open(db).context("open Photos.sqlite")?;
    let mut stmt = con.prepare(
        "SELECT ZDIRECTORY||'/'||ZFILENAME FROM ZASSET \
         WHERE ZTRASHEDSTATE=0 AND ZKIND=0 AND ZKINDSUBTYPE=2",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Extract identity keys of every non-trashed asset from Photos.sqlite.
pub fn iphone_keys(db: &Path) -> Result<Vec<MediaKey>> {
    let con = rusqlite::Connection::open(db).context("open Photos.sqlite")?;
    let mut stmt = con.prepare("PRAGMA table_info(ZASSET)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    let pick = |names: &[&str]| -> Option<String> {
        names
            .iter()
            .find(|n| cols.iter().any(|c| c == **n))
            .map(|s| s.to_string())
    };
    let c_dir = pick(&["ZDIRECTORY"]).context("ZDIRECTORY")?;
    let c_file = pick(&["ZFILENAME"]).context("ZFILENAME")?;
    let c_date = pick(&["ZDATECREATED", "ZADDEDDATE"]).context("ZDATECREATED")?;
    let c_w = pick(&["ZPIXELWIDTH", "ZWIDTH"]);
    let c_h = pick(&["ZPIXELHEIGHT", "ZHEIGHT"]);
    let w_expr = c_w.map(|c| format!("COALESCE({c},0)")).unwrap_or("0".into());
    let h_expr = c_h.map(|c| format!("COALESCE({c},0)")).unwrap_or("0".into());
    let trash = pick(&["ZTRASHEDSTATE"])
        .map(|c| format!("WHERE {c}=0"))
        .unwrap_or_default();
    let sql = format!(
        "SELECT {c_dir}||'/'||{c_file}, {c_date}, {w_expr}, {h_expr} FROM ZASSET {trash}"
    );
    let mut stmt = con.prepare(&sql)?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, f64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (path, cf, w, h) = r?;
        out.push(MediaKey {
            path,
            t: cf as i64 + CF_EPOCH, // absolute unix
            w: w as u32,
            h: h as u32,
        });
    }
    Ok(out)
}
