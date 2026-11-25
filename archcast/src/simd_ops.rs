//! SIMD-accelerated primitives for high-throughput HTML/SVG escaping,
//! UTF-8 / CJK display width estimation, and batched bounding-box queries.

use memchr::{memchr2, memchr3};
use wide::{CmpGe, CmpLe, f64x4};

/// Report compile-time SIMD acceleration profile active for this target.
pub fn simd_profile() -> &'static str {
    #[cfg(target_arch = "aarch64")]
    {
        "aarch64-neon (memchr + wide::f64x4)"
    }
    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    {
        "x86_64-avx2 (memchr + wide::f64x4)"
    }
    #[cfg(all(target_arch = "x86_64", not(target_feature = "avx2")))]
    {
        "x86_64-sse2 (memchr + wide::f64x4)"
    }
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    {
        "portable-simd (memchr + wide::f64x4)"
    }
}

/// Escape `&`, `<`, `>`, `"`, `'` into `out` using SIMD-accelerated `memchr` scanning
/// so clean ASCII/UTF-8 spans are bulk-copied with zero per-byte branching.
pub fn escape_html_into(input: &str, out: &mut String) {
    let bytes = input.as_bytes();
    let mut last = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        let rest = &bytes[i..];
        let next_special = match (
            memchr3(b'&', b'<', b'>', rest),
            memchr2(b'"', b'\'', rest),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        let Some(rel) = next_special else {
            break;
        };
        let pos = i + rel;
        if pos > last {
            out.push_str(&input[last..pos]);
        }
        match bytes[pos] {
            b'&' => out.push_str("&amp;"),
            b'<' => out.push_str("&lt;"),
            b'>' => out.push_str("&gt;"),
            b'"' => out.push_str("&quot;"),
            b'\'' => out.push_str("&#39;"),
            _ => {}
        }
        i = pos + 1;
        last = i;
    }

    if last < bytes.len() {
        out.push_str(&input[last..]);
    }
}

/// Convenience allocating wrapper around [`escape_html_into`].
pub fn escape_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 16);
    escape_html_into(input, &mut out);
    out
}

/// Estimate rendered pixel width of a mixed ASCII/CJK string for deterministic
/// SVG pill badge sizing and card layout without requiring system font files.
pub fn estimate_text_width_px(text: &str, font_size: f64, is_mono: bool) -> f64 {
    let mut units = 0.0_f64;
    for ch in text.chars() {
        if ch.is_ascii() {
            units += if is_mono {
                0.60
            } else if matches!(ch, 'i' | 'l' | 'I' | '.' | ',' | ':' | ';' | '|' | ' ') {
                0.34
            } else if ch.is_ascii_uppercase() || matches!(ch, 'm' | 'w' | 'M' | 'W') {
                0.68
            } else {
                0.55
            };
        } else if is_wide_cjk(ch) {
            units += 1.02;
        } else {
            units += 0.72;
        }
    }
    units * font_size
}

#[inline]
fn is_wide_cjk(ch: char) -> bool {
    let cp = ch as u32;
    matches!(
        cp,
        0x2E80..=0x9FFF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFF60
            | 0x20000..=0x2FA1F
    )
}

/// Structure-of-Arrays (SoA) bounding-box storage accelerated with `wide::f64x4`
/// for fast 4-wide point containment, card matching, and global canvas extent reduction.
#[derive(Debug, Clone, Default)]
pub struct BatchBounds {
    pub x0: Vec<f64>,
    pub y0: Vec<f64>,
    pub x1: Vec<f64>,
    pub y1: Vec<f64>,
}

impl BatchBounds {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            x0: Vec::with_capacity(cap),
            y0: Vec::with_capacity(cap),
            x1: Vec::with_capacity(cap),
            y1: Vec::with_capacity(cap),
        }
    }

    pub fn push(&mut self, x: f64, y: f64, w: f64, h: f64) -> usize {
        let idx = self.x0.len();
        self.x0.push(x);
        self.y0.push(y);
        self.x1.push(x + w);
        self.y1.push(y + h);
        idx
    }

    pub fn len(&self) -> usize {
        self.x0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.x0.is_empty()
    }

    /// Find the first box containing point `(px, py)` (expanded by `pad`),
    /// testing 4 boxes per SIMD step via `wide::f64x4`.
    pub fn find_containing(&self, px: f64, py: f64, pad: f64) -> Option<usize> {
        let n = self.x0.len();
        let vx = f64x4::splat(px);
        let vy = f64x4::splat(py);
        let vpad = f64x4::splat(pad);

        let mut i = 0usize;
        while i + 4 <= n {
            let bx0 = f64x4::new([self.x0[i], self.x0[i + 1], self.x0[i + 2], self.x0[i + 3]])
                - vpad;
            let by0 = f64x4::new([self.y0[i], self.y0[i + 1], self.y0[i + 2], self.y0[i + 3]])
                - vpad;
            let bx1 = f64x4::new([self.x1[i], self.x1[i + 1], self.x1[i + 2], self.x1[i + 3]])
                + vpad;
            let by1 = f64x4::new([self.y1[i], self.y1[i + 1], self.y1[i + 2], self.y1[i + 3]])
                + vpad;

            let hit = vx.cmp_ge(bx0) & vx.cmp_le(bx1) & vy.cmp_ge(by0) & vy.cmp_le(by1);
            let mask = hit.move_mask();
            if mask != 0 {
                return Some(i + mask.trailing_zeros() as usize);
            }
            i += 4;
        }

        while i < n {
            if px >= self.x0[i] - pad
                && px <= self.x1[i] + pad
                && py >= self.y0[i] - pad
                && py <= self.y1[i] + pad
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Compute global `(max_x, max_y)` across all boxes using 4-wide SIMD lane reduction.
    pub fn global_max_xy(&self) -> (f64, f64) {
        let n = self.x1.len();
        if n == 0 {
            return (0.0, 0.0);
        }
        let mut vmax_x = f64x4::splat(0.0);
        let mut vmax_y = f64x4::splat(0.0);
        let mut i = 0usize;
        while i + 4 <= n {
            let vx = f64x4::new([self.x1[i], self.x1[i + 1], self.x1[i + 2], self.x1[i + 3]]);
            let vy = f64x4::new([self.y1[i], self.y1[i + 1], self.y1[i + 2], self.y1[i + 3]]);
            vmax_x = vmax_x.max(vx);
            vmax_y = vmax_y.max(vy);
            i += 4;
        }
        let ax = vmax_x.to_array();
        let ay = vmax_y.to_array();
        let mut max_x = ax[0].max(ax[1]).max(ax[2]).max(ax[3]);
        let mut max_y = ay[0].max(ay[1]).max(ay[2]).max(ay[3]);
        while i < n {
            max_x = max_x.max(self.x1[i]);
            max_y = max_y.max(self.y1[i]);
            i += 1;
        }
        (max_x, max_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simd_escape_handles_mixed_utf8_and_entities() {
        let s = r#"方案 <v1> & "v2" 'test' → ok"#;
        assert_eq!(
            escape_html(s),
            "方案 &lt;v1&gt; &amp; &quot;v2&quot; &#39;test&#39; → ok"
        );
    }

    #[test]
    fn batch_bounds_simd_containment_and_max() {
        let mut b = BatchBounds::with_capacity(6);
        b.push(10.0, 20.0, 100.0, 50.0);
        b.push(200.0, 20.0, 100.0, 80.0);
        b.push(400.0, 50.0, 120.0, 90.0);
        b.push(600.0, 100.0, 200.0, 150.0);
        b.push(900.0, 300.0, 150.0, 200.0);

        assert_eq!(b.find_containing(50.0, 40.0, 0.0), Some(0));
        assert_eq!(b.find_containing(700.0, 180.0, 0.0), Some(3));
        assert_eq!(b.find_containing(950.0, 400.0, 0.0), Some(4));
        assert_eq!(b.find_containing(5.0, 5.0, 0.0), None);
        assert_eq!(b.global_max_xy(), (1050.0, 500.0));
    }
}
