use crunch::{Item, PackedItem, Packer, Rect, Rotation};

use crate::error::{AppError, Result};

/// Pack items into one or more bins, splitting automatically when needed.
/// Returns a vec of (bin_width, bin_height, packed_items) for each bin.
pub fn pack_multi_bin(
    items: Vec<(String, usize, usize)>, // (name, width, height)
    max_size: usize,
    pot: bool,
    allow_rotation: bool,
) -> Result<Vec<(usize, usize, Vec<PackedItem<String>>)>> {
    if items.is_empty() {
        return Ok(vec![]);
    }

    // Validate all items fit within max_size individually
    for (name, w, h) in &items {
        let min_dim = if allow_rotation {
            (*w).min(*h)
        } else {
            *w
        };
        let max_dim = if allow_rotation {
            (*w).max(*h)
        } else {
            *h
        };
        if min_dim > max_size || max_dim > max_size {
            return Err(AppError::InvalidParam(format!(
                "Sprite '{}' ({}x{}) exceeds max atlas size {}",
                name, w, h, max_size
            )));
        }
    }

    let rot = if allow_rotation {
        Rotation::Allowed
    } else {
        Rotation::None
    };

    let mut remaining: Vec<Item<String>> = items
        .into_iter()
        .map(|(name, w, h)| Item::new(name, w, h, rot))
        .collect();

    let mut bins = Vec::new();

    while !remaining.is_empty() {
        let mut packer = Packer::with_items(remaining.clone());

        if pot {
            match packer.pack_into_po2(max_size) {
                Ok(packed) => {
                    bins.push((packed.w, packed.h, packed.items));
                    return Ok(bins);
                }
                Err(()) => {
                    // Try packing into max_size to see how many fit
                }
            }
        } else {
            // Try to fit all into an auto-sized rect
            if let Some(result) = try_auto_size(&remaining, max_size) {
                bins.push(result);
                return Ok(bins);
            }
        }

        // Can't fit all — pack as many as possible into max_size
        let rect = Rect::of_size(max_size, max_size);
        let mut packer = Packer::with_items(remaining.clone());
        let packed = match packer.pack(rect) {
            Ok(all) => all,
            Err(partial) => partial,
        };

        if packed.is_empty() {
            return Err(AppError::PackingFailed(max_size));
        }

        // Determine which items were packed
        let packed_names: std::collections::HashSet<&str> =
            packed.iter().map(|p| p.data.as_str()).collect();

        remaining.retain(|item| !packed_names.contains(item.data.as_str()));

        // Compute tight bounding box
        let (actual_w, actual_h) = tight_bounds(&packed);
        let (final_w, final_h) = if pot {
            (next_pot(actual_w), next_pot(actual_h))
        } else {
            (actual_w, actual_h)
        };

        bins.push((final_w, final_h, packed));
    }

    Ok(bins)
}

/// Try to fit all items into an auto-sized non-POT atlas.
fn try_auto_size(
    items: &[Item<String>],
    max_size: usize,
) -> Option<(usize, usize, Vec<PackedItem<String>>)> {
    let total_area: usize = items.iter().map(|i| i.w * i.h).sum();
    let side = (total_area as f64).sqrt().ceil() as usize;

    // Try a few sizes
    for mult_num in [10, 12, 14, 16, 20] {
        let try_size = ((side * mult_num) / 10).min(max_size);
        if try_size < 1 {
            continue;
        }

        // Try square first, then wider, then taller
        for (w, h) in [
            (try_size, try_size),
            ((try_size * 3 / 2).min(max_size), try_size),
            (try_size, (try_size * 3 / 2).min(max_size)),
        ] {
            let mut packer = Packer::with_items(items.iter().cloned());
            if let Ok(packed) = packer.pack(Rect::of_size(w, h)) {
                let (actual_w, actual_h) = tight_bounds(&packed);
                return Some((actual_w, actual_h, packed));
            }
        }
    }

    None
}

/// Compute tight bounding box around packed items.
fn tight_bounds(packed: &[PackedItem<String>]) -> (usize, usize) {
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    for item in packed {
        max_x = max_x.max(item.rect.x + item.rect.w);
        max_y = max_y.max(item.rect.y + item.rect.h);
    }
    (max_x.max(1), max_y.max(1))
}

/// Next power of 2 >= n.
fn next_pot(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut v = n - 1;
    v |= v >> 1;
    v |= v >> 2;
    v |= v >> 4;
    v |= v >> 8;
    v |= v >> 16;
    v + 1
}
