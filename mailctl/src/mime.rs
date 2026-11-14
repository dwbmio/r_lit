//! MIME 组装与解析：base64 正文、RFC 2047 头编码/解码、收件人提取。

use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};

const B64: base64::engine::general_purpose::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub struct MessageSpec<'a> {
    pub from_name: Option<&'a str>,
    pub from_addr: &'a str,
    pub to: &'a str,
    pub cc: Option<&'a str>,
    pub subject: &'a str,
    pub body: &'a str,
    pub in_reply_to: Option<&'a str>,
    pub references: Option<&'a str>,
}

pub struct BuiltMessage {
    pub raw: Vec<u8>,
    pub message_id: String,
}

pub fn is_ascii_printable(s: &str) -> bool {
    s.bytes().all(|b| (0x20..0x7f).contains(&b))
}

/// RFC 2047 B 编码；纯 ASCII 且无特殊字符时原样返回。
pub fn encode_header_value(s: &str) -> String {
    if is_ascii_printable(s) && !s.contains("=?") {
        return s.to_string();
    }
    // 按 45 字节切块，避免超长 encoded-word
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut end = (i + 45).min(bytes.len());
        while end > i && !s.is_char_boundary(end) {
            end -= 1;
        }
        if end == i {
            end = bytes.len();
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("=?UTF-8?B?{}?=", B64.encode(&bytes[i..end])));
        i = end;
    }
    out
}

/// 解 RFC 2047（B 与 Q），相邻 encoded-word 拼接。
pub fn decode_rfc2047(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s.trim();
    while let Some(pos) = rest.find("=?") {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        match parse_encoded_word(tail) {
            Some((consumed, decoded)) => {
                out.push_str(&decoded);
                rest = &tail[consumed..];
                // 相邻 encoded-word 之间的空白应吞掉
                if rest.starts_with(' ') && rest[1..].starts_with("=?") {
                    rest = rest[1..].trim_start();
                }
            }
            None => {
                out.push_str("=?");
                rest = &tail[2..];
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// `=?charset?B|Q?payload?=` → (消耗字节数, 解码结果)
fn parse_encoded_word(tail: &str) -> Option<(usize, String)> {
    let body = tail.strip_prefix("=?")?;
    let p1 = body.find('?')?;
    let charset = &body[..p1];
    let after = &body[p1 + 1..];
    let mut chars = after.chars();
    let enc = chars.next()?.to_ascii_uppercase();
    if !matches!(enc, 'B' | 'Q') || after.as_bytes().get(1) != Some(&b'?') {
        return None;
    }
    let payload_rest = &after[2..];
    let p2 = payload_rest.find("?=")?;
    let payload = &payload_rest[..p2];
    let decoded: Vec<u8> = match enc {
        'B' => B64.decode(payload.trim()).unwrap_or_else(|_| payload.as_bytes().to_vec()),
        _ => decode_q(payload),
    };
    let consumed = 2 + p1 + 1 + 2 + p2 + 2;
    Some((consumed, decode_bytes(&decoded, charset)))
}

fn decode_q(payload: &str) -> Vec<u8> {
    let b = payload.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'_' => out.push(b' '),
            b'=' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b[i]),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    out
}


/// 按 charset 解码字节（GBK/GB18030/Big5/UTF-8…），未知 charset 退回 UTF-8 有损。
pub fn decode_bytes(data: &[u8], charset: &str) -> String {
    let label = charset.trim().trim_matches('"').to_ascii_lowercase();
    if label.is_empty() || label == "utf-8" || label == "utf8" || label == "us-ascii" || label == "ascii" {
        return String::from_utf8_lossy(data).into_owned();
    }
    if let Some(enc) = encoding_rs::Encoding::for_label(label.as_bytes()) {
        let (text, _, _) = enc.decode(data);
        return text.into_owned();
    }
    String::from_utf8_lossy(data).into_owned()
}

/// 从原始邮件取纯文本正文（支持嵌套 multipart、base64 / quoted-printable、多字符集）
pub fn extract_text(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).into_owned();
    extract_from_entity(&text, 0)
}

/// 把折行头部展开成单行（续行以空格/制表符开头）
fn unfold(head: &str) -> String {
    let mut out = String::with_capacity(head.len());
    for line in head.split("\r\n").flat_map(|l| l.split('\n')) {
        if line.starts_with(' ') || line.starts_with('\t') {
            out.push(' ');
            out.push_str(line.trim_start());
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
    }
    out
}

fn split_head_body(entity: &str) -> (String, String) {
    // 邮件常混用 CRLF / LF：取「最先出现的空行」作为分界
    let (split_at, sep_len) = match (entity.find("\r\n\r\n"), entity.find("\n\n")) {
        (Some(a), Some(b)) if b < a => (b, 2),
        (Some(a), _) => (a, 4),
        (None, Some(b)) => (b, 2),
        (None, None) => (entity.len(), 0),
    };
    let head = entity[..split_at].to_string();
    let body = if sep_len == 0 { String::new() } else { entity[split_at + sep_len..].to_string() };
    (head, body)
}

fn extract_from_entity(entity: &str, depth: usize) -> String {
    if depth > 5 {
        return String::new();
    }
    let (head_raw, body) = split_head_body(entity);
    let head = unfold(&head_raw);
    let ctype = header_value(&head, "Content-Type").unwrap_or_default();
    let cte = header_value(&head, "Content-Transfer-Encoding").unwrap_or_default();
    let type_l = ctype.to_ascii_lowercase();
    let charset = charset_of(&ctype).unwrap_or_else(|| "utf-8".into());

    let boundary = boundary_of(&ctype).or_else(|| detect_boundary(&body));
    let is_multipart = type_l.starts_with("multipart/")
        || (type_l.contains("multipart") && boundary.is_some())
        || (type_l.is_empty() && boundary.is_some());

    if is_multipart {
        if let Some(boundary) = boundary {
            let mut html_fallback = String::new();
            for part in body.split(&format!("--{boundary}")) {
                let trimmed = part.trim_start();
                if trimmed.is_empty() || trimmed.starts_with("--") {
                    continue; // 前导/结束分隔
                }
                let (ph_raw, pb) = split_head_body(part);
                let ph = unfold(&ph_raw);
                let ptype = header_value(&ph, "Content-Type").unwrap_or_default().to_ascii_lowercase();
                if ptype.starts_with("multipart/") {
                    let nested = extract_from_entity(part, depth + 1);
                    if !nested.trim().is_empty() {
                        return nested;
                    }
                    continue;
                }
                if ptype.contains("text/html") {
                    if html_fallback.trim().is_empty() {
                        let pcte = header_value(&ph, "Content-Transfer-Encoding").unwrap_or_default();
                        let pcharset = charset_of(&ptype).unwrap_or_else(|| charset.clone());
                        html_fallback = strip_html(&decode_body(&pb, &pcte, &pcharset));
                    }
                    continue;
                }
                // 默认当 text/plain（Apple 等厂商常把 plain 段做得像 HTML，仍按原样返回）
                let pcte = header_value(&ph, "Content-Transfer-Encoding").unwrap_or_default();
                let pcharset = charset_of(&ptype).unwrap_or_else(|| charset.clone());
                let decoded = decode_body(&pb, &pcte, &pcharset);
                if !decoded.trim().is_empty() {
                    return decoded;
                }
            }
            return html_fallback;
        }
    }
    decode_body(&body, &cte, &charset)
}

/// 极简去标签：text/html 兜底时用
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn header_value(head: &str, name: &str) -> Option<String> {
    for line in head.split("\r\n").flat_map(|l| l.split('\n')) {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn charset_of(ctype: &str) -> Option<String> {
    let lower = ctype.to_ascii_lowercase();
    let idx = lower.find("charset=")?;
    let rest = &ctype[idx + 8..];
    let val = rest.split([';', ' ', '\r', '\n']).next()?.trim().trim_matches('"');
    if val.is_empty() {
        None
    } else {
        Some(val.to_string())
    }
}

/// 头部没给 boundary 时，从正文里猜：找形如 `--xxxx` 且出现≥2次的分隔行
fn detect_boundary(body: &str) -> Option<String> {
    for line in body.lines() {
        let l = line.trim_end_matches('\r');
        let Some(rest) = l.strip_prefix("--") else { continue };
        let cand = rest.trim_end_matches('-');
        if cand.len() < 8 {
            continue;
        }
        let needle = format!("--{cand}");
        if body.matches(&needle).count() >= 2 {
            return Some(cand.to_string());
        }
    }
    None
}

fn boundary_of(ctype: &str) -> Option<String> {
    let lower = ctype.to_ascii_lowercase();
    let idx = lower.find("boundary=")?;
    let rest = &ctype[idx + 9..];
    let val = rest
        .split([';', '\r', '\n'])
        .next()?
        .trim()
        .trim_matches('"')
        .to_string();
    if val.is_empty() {
        None
    } else {
        Some(val)
    }
}

fn decode_qp_body(body: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            // 软换行 =\r\n / =\n
            if i + 1 < bytes.len() && (bytes[i + 1] == b'\n' || bytes[i + 1] == b'\r') {
                i += if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') { 3 } else { 2 };
                continue;
            }
            if i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn decode_body(body: &str, cte: &str, charset: &str) -> String {
    let cte_l = cte.to_ascii_lowercase();
    if cte_l.contains("base64") {
        let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(bytes) = B64.decode(cleaned) {
            return decode_bytes(&bytes, charset);
        }
    }
    if cte_l.contains("quoted-printable") {
        return decode_bytes(&decode_qp_body(body), charset);
    }
    decode_bytes(body.as_bytes(), charset)
}

pub fn build(spec: &MessageSpec<'_>) -> BuiltMessage {
    let message_id = make_message_id(spec.from_addr);
    let mut h = String::new();
    let from = match spec.from_name {
        Some(n) if !n.is_empty() => format!("{} <{}>", encode_header_value(n), spec.from_addr),
        _ => spec.from_addr.to_string(),
    };
    h.push_str(&format!("From: {from}\r\n"));
    h.push_str(&format!("To: {}\r\n", spec.to));
    if let Some(cc) = spec.cc.filter(|c| !c.trim().is_empty()) {
        h.push_str(&format!("Cc: {cc}\r\n"));
    }
    h.push_str(&format!("Subject: {}\r\n", encode_header_value(spec.subject)));
    h.push_str(&format!("Date: {}\r\n", rfc2822_now()));
    h.push_str(&format!("Message-ID: {message_id}\r\n"));
    if let Some(irt) = spec.in_reply_to.filter(|s| !s.trim().is_empty()) {
        h.push_str(&format!("In-Reply-To: {}\r\n", irt.trim()));
    }
    let refs = [spec.references.unwrap_or("").trim(), spec.in_reply_to.unwrap_or("").trim()]
        .iter()
        .filter(|s| !s.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if !refs.is_empty() {
        h.push_str(&format!("References: {refs}\r\n"));
    }
    h.push_str("MIME-Version: 1.0\r\n");
    h.push_str("Content-Type: text/plain; charset=utf-8\r\n");
    h.push_str("Content-Transfer-Encoding: base64\r\n");
    h.push_str("X-Mailer: mailctl/0.1 (rust)\r\n");
    h.push_str("\r\n");

    let mut raw = h.into_bytes();
    raw.extend_from_slice(wrap_b64(spec.body.as_bytes()).as_bytes());
    BuiltMessage { raw, message_id }
}

fn wrap_b64(data: &[u8]) -> String {
    let enc = B64.encode(data);
    let mut out = String::with_capacity(enc.len() + enc.len() / 76 + 2);
    let bytes = enc.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let end = (i + 76).min(bytes.len());
        out.push_str(std::str::from_utf8(&bytes[i..end]).unwrap_or(""));
        out.push_str("\r\n");
        i = end;
    }
    out
}

fn make_message_id(from_addr: &str) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let domain = from_addr.rsplit('@').next().unwrap_or("localhost");
    format!("<{nanos}.{}@{domain}>", std::process::id())
}

pub fn rfc2822_now() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let fmt = time::format_description::well_known::Rfc2822;
    now.format(&fmt).unwrap_or_else(|_| String::from("Thu, 01 Jan 1970 00:00:00 +0000"))
}

/// 从 To/Cc 头里提取地址列表
/// `Anny <a@b>` → 可安全写入头部的 `Anny <a@b>`（显示名按需 RFC2047 编码）
pub fn format_address(header_value: &str) -> String {
    let v = header_value.trim();
    if let (Some(s), Some(e)) = (v.rfind('<'), v.rfind('>')) {
        if s < e {
            let name = decode_rfc2047(v[..s].trim().trim_matches('"'));
            let addr = v[s + 1..e].trim();
            if name.is_empty() {
                return addr.to_string();
            }
            return format!("{} <{}>", encode_header_value(&name), addr);
        }
    }
    v.to_string()
}

pub fn parse_recipients(header: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in header.split(',') {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        let addr = if let (Some(s), Some(e)) = (p.rfind('<'), p.rfind('>')) {
            if s < e {
                p[s + 1..e].trim().to_string()
            } else {
                p.to_string()
            }
        } else {
            p.to_string()
        };
        if addr.contains('@') {
            out.push(addr);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc2047_roundtrip() {
        let s = "关于注册申请 ZY8HUL67D3";
        let enc = encode_header_value(s);
        assert!(enc.starts_with("=?UTF-8?B?"));
        assert_eq!(decode_rfc2047(&enc), s);
    }

    #[test]
    fn q_encoding_decodes() {
        assert_eq!(decode_rfc2047("=?utf-8?Q?=E5=85=B3=E4=BA=8E?="), "关于");
    }

    #[test]
    fn adjacent_encoded_words_join() {
        assert_eq!(decode_rfc2047("=?utf-8?B?5YWz5LqO?= =?utf-8?B?5rOo5YaM?="), "关于注册");
    }

    #[test]
    fn plain_subject_untouched() {
        assert_eq!(decode_rfc2047("Re: hello ZY8HUL67D3"), "Re: hello ZY8HUL67D3");
    }

    #[test]
    fn built_message_has_thread_headers() {
        let spec = MessageSpec {
            from_name: Some("段文彬 | Mujin Technology"),
            from_addr: "a@qq.com",
            to: "Anny <developer@apple.com>",
            cc: None,
            subject: "Re: 注册 ID ZY8HUL67D3",
            body: "Anny 你好：材料已上传。",
            in_reply_to: Some("<CASE-1@apple.com>"),
            references: Some("<ROOT@apple.com>"),
        };
        let m = build(&spec);
        let text = String::from_utf8_lossy(&m.raw);
        assert!(text.contains("In-Reply-To: <CASE-1@apple.com>"));
        assert!(text.contains("References: <ROOT@apple.com> <CASE-1@apple.com>"));
        assert!(text.contains("Content-Transfer-Encoding: base64"));
    }

    #[test]
    fn extracts_text_from_nested_multipart() {
        let raw = concat!(
            "From: a@b.c\n",
            "Subject: t\n",
            "Content-Type: multipart/mixed; boundary=\"Outer\"\n",
            "\n",
            "--Outer\n",
            "Content-Type: multipart/alternative; boundary=\"Inner\"\n",
            "\n",
            "--Inner\n",
            "Content-Type: text/plain; charset=UTF-8\n",
            "Content-Transfer-Encoding: BASE64\n",
            "\n",
            "5L2g5aW977ya5p2Q5paZ5bey5LiK5Lyg\n",
            "--Inner--\n",
            "--Outer--\n",
        );
        let t = extract_text(raw.as_bytes());
        assert!(t.contains("你好"), "got: {t}");
    }

    #[test]
    fn decodes_gbk_encoded_word() {
        // "=?...?B?1tDOxA==?=" 是把「中文」的 GBK 字节 0xD6D0 0xCEC4 做 base64
        let s = "=?gb2312?B?1tDOxA==?=";
        assert_eq!(decode_rfc2047(s), "中文");
    }

    #[test]
    fn format_address_keeps_display_name() {
        assert_eq!(format_address("Anny <developer@apple.com>"), "Anny <developer@apple.com>");
        assert_eq!(format_address("developer@apple.com"), "developer@apple.com");
    }

    #[test]
    fn recipients_parse() {
        let r = parse_recipients("Anny <developer@apple.com>, other@x.com");
        assert_eq!(r, vec!["developer@apple.com", "other@x.com"]);
    }

    #[test]
    fn dot_stuffing_prefixes_leading_dot() {
        let s = super::super::smtp::dot_stuff_pub(b".hidden\r\n..x\r\n");
        assert_eq!(s, b"..hidden\r\n...x\r\n".to_vec());
    }
}
