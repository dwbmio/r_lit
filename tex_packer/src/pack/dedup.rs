use image::RgbaImage;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Cheap pre-hash: dimensions + first/last/center pixel samples.
/// Different cheap_key = definitely different. Same cheap_key = maybe same, need full hash.
#[inline]
fn cheap_key(img: &RgbaImage) -> u64 {
    let (w, h) = img.dimensions();
    let raw = img.as_raw();
    let len = raw.len();

    let mut key = (w as u64) << 32 | (h as u64);

    // Sample a few bytes from start, middle, end
    if len >= 16 {
        let s = &raw[0..8];
        key ^= u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
    }
    if len >= 32 {
        let mid = len / 2;
        let s = &raw[mid..mid + 8];
        key ^= u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]).rotate_left(17);
    }
    if len >= 16 {
        let s = &raw[len - 8..];
        key ^= u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]).rotate_left(31);
    }

    key
}

/// Full SHA256 hash of pixel data.
fn pixel_hash(img: &RgbaImage) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(img.width().to_le_bytes());
    hasher.update(img.height().to_le_bytes());
    hasher.update(img.as_raw());
    hasher.finalize().into()
}

/// Find duplicates using two-phase approach:
/// Phase 1: cheap_key groups (instant) — different key = skip full hash
/// Phase 2: SHA256 only for same-key groups
/// Returns (unique indices, alias map).
pub fn find_duplicates(sprites: &[(String, RgbaImage)]) -> (Vec<usize>, HashMap<String, String>) {
    // Phase 1: group by cheap_key
    let mut key_groups: HashMap<u64, Vec<usize>> = HashMap::new();
    for (i, (_name, img)) in sprites.iter().enumerate() {
        let key = cheap_key(img);
        key_groups.entry(key).or_default().push(i);
    }

    let mut unique_indices = Vec::new();
    let mut aliases: HashMap<String, String> = HashMap::new();

    // Phase 2: only full-hash within same-key groups
    // hash → (first index, canonical name)
    let mut seen_full: HashMap<[u8; 32], (usize, String)> = HashMap::new();

    for (_key, group) in &key_groups {
        if group.len() == 1 {
            // Unique cheap_key — no possible duplicate, skip SHA256
            unique_indices.push(group[0]);
            continue;
        }

        // Multiple sprites with same cheap_key — full hash to confirm
        for &i in group {
            let (name, img) = &sprites[i];
            let hash = pixel_hash(img);

            if let Some((_first_idx, canonical_name)) = seen_full.get(&hash) {
                aliases.insert(name.clone(), canonical_name.clone());
                log::info!("Duplicate: '{}' == '{}' (skipped)", name, canonical_name);
            } else {
                seen_full.insert(hash, (i, name.clone()));
                unique_indices.push(i);
            }
        }
    }

    let dup_count = aliases.len();
    if dup_count > 0 {
        log::info!(
            "Dedup: {} duplicate(s) found, {} unique sprite(s) to pack",
            dup_count,
            unique_indices.len()
        );
    }

    // Sort to keep deterministic order
    unique_indices.sort();

    (unique_indices, aliases)
}
