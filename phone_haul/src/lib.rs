//! Shared protocol between phone_haul (host) and phone_haul_agent (device).
//!
//! Stream framing (agent -> host, little-endian):
//!   magic u32 = 0x4144_4831 ("ADH1")
//!   repeated records:
//!     path_len u32  (0 => end of stream)
//!     path bytes [u8; path_len]
//!     size u64
//!     payload [u8; size]

use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

pub const STREAM_MAGIC: u32 = 0x4144_4831;
pub const QUICK_WINDOW: usize = 64 * 1024;
pub const SNIFF_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Jpeg,
    Png,
    Gif,
    Webp,
    Heic,
    Avif,
    Dng,
    Mp4,
    Mov,
    Webm,
    Other,
}

impl MediaKind {
    pub fn is_image(self) -> bool {
        matches!(
            self,
            MediaKind::Jpeg
                | MediaKind::Png
                | MediaKind::Gif
                | MediaKind::Webp
                | MediaKind::Heic
                | MediaKind::Avif
                | MediaKind::Dng
        )
    }
}

/// Content-sniffing classifier: magic bytes only, never file extension.
pub fn classify(head: &[u8]) -> MediaKind {
    if head.len() >= 3 && head[0] == 0xFF && head[1] == 0xD8 && head[2] == 0xFF {
        return MediaKind::Jpeg;
    }
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return MediaKind::Png;
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return MediaKind::Gif;
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return MediaKind::Webp;
    }
    // ISO BMFF family: ftyp box at offset 4
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        return match brand {
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" => MediaKind::Heic,
            b"avif" | b"avis" => MediaKind::Avif,
            b"qt  " => MediaKind::Mov,
            _ => MediaKind::Mp4, // isom/mp41/mp42/M4V/3gp...
        };
    }
    if head.len() >= 4 && &head[0..4] == b"\x1a\x45\xdf\xa3" {
        return MediaKind::Webm;
    }
    MediaKind::Other
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub mtime: i64,
    pub kind: MediaKind,
    /// blake3 hex of (first/mid/last 64KiB + size) — cheap device-side pre-filter
    pub quick: String,
}

/// Two-tier dedup tier-1: sample head/mid/tail windows + length.
/// Tier-2 (authoritative) is the full BLAKE3 computed while streaming on host.
pub fn quick_hash(f: &mut File, size: u64) -> io::Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(&size.to_le_bytes());
    let mut buf = vec![0u8; QUICK_WINDOW];
    let mid = size / 2;
    let tail = size.saturating_sub(QUICK_WINDOW as u64);
    for off in [0u64, mid, tail] {
        f.seek(SeekFrom::Start(off))?;
        let n = read_up_to(f, &mut buf)?;
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

fn read_up_to(r: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match r.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_magic() {
        assert_eq!(classify(&[0xFF, 0xD8, 0xFF, 0xE0]), MediaKind::Jpeg);
        assert_eq!(classify(b"\x89PNG\r\n\x1a\n...."), MediaKind::Png);
        assert_eq!(classify(b"GIF89a...."), MediaKind::Gif);
        assert_eq!(
            classify(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            MediaKind::Webp
        );
        assert_eq!(
            classify(b"\x00\x00\x00\x18ftypheic\x00"),
            MediaKind::Heic
        );
        assert_eq!(classify(b"\x00\x00\x00\x18ftypisom\x00"), MediaKind::Mp4);
        assert_eq!(classify(b"plain text"), MediaKind::Other);
        assert!(MediaKind::Jpeg.is_image());
        assert!(!MediaKind::Mp4.is_image());
    }
}
