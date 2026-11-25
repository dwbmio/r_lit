use anyhow::Result;
use std::io::Write;

/// Copy text to the local OS clipboard using Pure Rust (`arboard`).
/// Falls back to an OSC 52 terminal escape sequence if the OS clipboard is unavailable (e.g. headless SSH).
pub fn copy_to_clipboard(text: &str) -> Result<String> {
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.to_string())) {
        Ok(()) => Ok("系统剪贴板".to_string()),
        Err(os_err) => {
            let encoded = b64_encode(text.as_bytes());
            let mut stdout = std::io::stdout();
            if write!(stdout, "\x1b]52;c;{encoded}\x07").is_ok() && stdout.flush().is_ok() {
                Ok("终端剪贴板 (OSC52)".to_string())
            } else {
                Err(anyhow::anyhow!("写入剪贴板失败: {os_err}"))
            }
        }
    }
}

fn b64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_encodes_utf8() {
        assert_eq!(b64_encode(b"hello"), "aGVsbG8=");
        assert_eq!(b64_encode("结论".as_bytes()), "57uT6K66");
    }
}
