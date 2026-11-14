//! 极简 IMAP 客户端：登录、只读打开、UID SEARCH、取头部、APPEND。
//! 只实现本工具用到的命令，不做通用库。

use crate::config::{Resolved, Security};
use crate::net::{connect, Conn, Stream};
use anyhow::{anyhow, bail, Result};
use base64::Engine as _;

pub struct Imap {
    conn: Conn,
    tag: u32,
    host: String,
}

pub struct HeaderHit {
    pub uid: u32,
    pub raw: Vec<u8>,
}

impl Imap {
    pub fn connect(r: &Resolved) -> Result<Imap> {
        let stream: Stream = connect(&r.imap_host, r.imap_port, r.imap_security)?;
        let mut conn = Conn::new(stream);
        let greeting = conn.read_line()?;
        if !greeting.starts_with("* OK") && !greeting.starts_with("* PREAUTH") {
            bail!("IMAP 问候异常：{greeting}");
        }
        let mut imap = Imap { conn, tag: 0, host: r.imap_host.clone() };
        if r.imap_security == Security::StartTls {
            let resp = imap.cmd("STARTTLS")?;
            if !resp.contains(" OK") {
                bail!("STARTTLS 被拒：{resp}");
            }
            imap.conn.upgrade_tls(&r.imap_host)?;
        }
        Ok(imap)
    }

    fn next_tag(&mut self) -> String {
        self.tag += 1;
        format!("A{:04}", self.tag)
    }

    /// 发一条命令，读到带同 tag 的结束行；返回期间所有行（含未标记行）。
    fn cmd(&mut self, text: &str) -> Result<String> {
        let tag = self.next_tag();
        self.conn.write_line(&format!("{tag} {text}"))?;
        let mut acc = String::new();
        loop {
            let line = self.conn.read_line()?;
            let done = line.starts_with(&format!("{tag} "));
            acc.push_str(&line);
            acc.push('\n');
            if done {
                if !line.contains(" OK") {
                    return Ok(acc); // 调用方按内容判断失败原因
                }
                return Ok(acc);
            }
        }
    }

    pub fn login(&mut self, user: &str, pass: &str) -> Result<()> {
        let resp = self.cmd(&format!("LOGIN {} {}", quote(user), quote(pass)))?;
        if !last_line_ok(&resp) {
            bail!("IMAP 登录失败（多半是授权码/应用密码不对，或没开 IMAP）：{resp}");
        }
        Ok(())
    }

    /// EXAMINE INBOX（只读），返回邮件数。
    pub fn examine_inbox(&mut self) -> Result<usize> {
        self.examine("INBOX")
    }

    /// EXAMINE <folder>（只读），返回邮件数。用于 INBOX 或「已发送」目录回读。
    pub fn examine(&mut self, folder: &str) -> Result<usize> {
        let resp = self.cmd(&format!("EXAMINE {}", quote(folder)))?;
        if !last_line_ok(&resp) {
            bail!("EXAMINE {folder} 失败：{resp}");
        }
        let mut count = 0;
        for line in resp.lines() {
            if let Some(rest) = line.strip_prefix("* ") {
                if let Some(num) = rest.split_whitespace().next() {
                    if rest.to_ascii_uppercase().contains("EXISTS") {
                        count = num.parse().unwrap_or(0);
                    }
                }
            }
        }
        Ok(count)
    }

    /// LIST "" "*" → 目录名列表（含 IMAP modified-UTF-7 解码后的可读名）
    pub fn list_folders(&mut self) -> Result<Vec<(String, String)>> {
        let resp = self.cmd("LIST \"\" \"*\"")?;
        let mut out = Vec::new();
        for line in resp.lines() {
            let Some(rest) = line.strip_prefix("* LIST ") else { continue };
            let parts: Vec<&str> = rest.split('"').collect();
            let raw = if parts.len() >= 4 {
                parts[3].to_string()
            } else {
                rest.split_whitespace().last().unwrap_or("").trim_matches('"').to_string()
            };
            if raw.is_empty() {
                continue;
            }
            out.push((raw.clone(), decode_imap_utf7(&raw)));
        }
        Ok(out)
    }

    /// UID SEARCH ALL → uid 列表（升序）
    pub fn uid_search_all(&mut self) -> Result<Vec<u32>> {
        self.uid_search("ALL")
    }

    /// UID SEARCH <criteria>（如 ALL / UNSEEN）
    pub fn uid_search(&mut self, criteria: &str) -> Result<Vec<u32>> {
        let resp = self.cmd(&format!("UID SEARCH {criteria}"))?;
        if !last_line_ok(&resp) {
            bail!("UID SEARCH {criteria} 失败：{resp}");
        }
        let mut uids = Vec::new();
        for line in resp.lines() {
            if let Some(rest) = line.strip_prefix("* SEARCH") {
                for tok in rest.split_whitespace() {
                    if let Ok(v) = tok.parse::<u32>() {
                        uids.push(v);
                    }
                }
            }
        }
        Ok(uids)
    }

    /// UID FETCH <uid> (BODY.PEEK[HEADER]) → 原始头部字节
    pub fn fetch_header(&mut self, uid: u32) -> Result<Vec<u8>> {
        let tag = self.next_tag();
        self.conn
            .write_line(&format!("{tag} UID FETCH {uid} (BODY.PEEK[HEADER])"))?;
        let mut header = Vec::new();
        loop {
            let line = self.conn.read_line()?;
            if line.starts_with(&format!("{tag} ")) {
                if !line.contains(" OK") {
                    bail!("UID FETCH {uid} 失败：{line}");
                }
                break;
            }
            if let Some(n) = literal_len(&line) {
                header = self.conn.read_exact_len(n)?;
            }
        }
        if header.is_empty() {
            bail!("UID {uid} 头部为空");
        }
        Ok(header)
    }

    /// UID FETCH <uid> (BODY.PEEK[]) → 整封原始邮件
    pub fn fetch_full(&mut self, uid: u32) -> Result<Vec<u8>> {
        let tag = self.next_tag();
        self.conn.write_line(&format!("{tag} UID FETCH {uid} (BODY.PEEK[])"))?;
        let mut raw = Vec::new();
        loop {
            let line = self.conn.read_line()?;
            if line.starts_with(&format!("{tag} ")) {
                if !line.contains(" OK") {
                    bail!("UID FETCH {uid} (BODY.PEEK[]) 失败：{line}");
                }
                break;
            }
            if let Some(n) = literal_len(&line) {
                raw = self.conn.read_exact_len(n)?;
            }
        }
        if raw.is_empty() {
            bail!("UID {uid} 内容为空");
        }
        Ok(raw)
    }

    /// APPEND 到已发送目录（失败不影响已发出的邮件）
    pub fn append(&mut self, folder: &str, bytes: &[u8]) -> Result<String> {
        let tag = self.next_tag();
        self.conn.write_line(&format!(
            "{tag} APPEND {} (\\Seen) {{{}}}",
            quote(folder),
            bytes.len()
        ))?;
        let cont = self.conn.read_line()?;
        if !cont.starts_with('+') {
            bail!("APPEND 未获续行许可：{cont}");
        }
        self.conn.write_raw(bytes)?;
        self.conn.write_raw(b"\r\n")?;
        let mut acc = String::new();
        loop {
            let line = self.conn.read_line()?;
            let done = line.starts_with(&format!("{tag} "));
            acc.push_str(&line);
            if done {
                return Ok(acc);
            }
            acc.push('\n');
        }
    }

    pub fn logout(&mut self) {
        let _ = self.cmd("LOGOUT");
    }

    pub fn host(&self) -> &str {
        &self.host
    }
}

/// IMAP modified UTF-7（RFC 3501 §5.1.3）→ UTF-8，用于把 `&XfJT0ZAB-` 显示成「已发送」
pub fn decode_imap_utf7(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                out.push('&');
                i += 2;
                continue;
            }
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'-' {
                j += 1;
            }
            let chunk: String = s[i + 1..j].replace(',', "/");
            let padded = match chunk.len() % 4 {
                2 => format!("{chunk}=="),
                3 => format!("{chunk}="),
                _ => chunk,
            };
            if let Ok(dec) = base64::engine::general_purpose::STANDARD.decode(padded) {
                let units: Vec<u16> = dec
                    .chunks_exact(2)
                    .map(|c| u16::from_be_bytes([c[0], c[1]]))
                    .collect();
                out.push_str(&String::from_utf16_lossy(&units));
            }
            i = if j < bytes.len() { j + 1 } else { j };
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn last_line_ok(resp: &str) -> bool {
    resp.lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .map(|l| l.contains(" OK"))
        .unwrap_or(false)
}

fn literal_len(line: &str) -> Option<usize> {
    let l = line.trim_end();
    if !l.ends_with('}') {
        return None;
    }
    let open = l.rfind('{')?;
    l.get(open + 1..l.len() - 1)?.parse::<usize>().ok()
}

pub fn decode_header_value(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    for line in text.split("\r\n") {
        let Some((k, v)) = line.split_once(':') else { continue };
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(crate::mime::decode_rfc2047(v.trim()));
        }
    }
    None
}

/// 收集某头部（含折行续行）
pub fn header_unfolded(raw: &[u8], name: &str) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let lines: Vec<&str> = text.split("\r\n").collect();
    let mut out: Option<String> = None;
    for line in lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(ref mut acc) = out {
                acc.push(' ');
                acc.push_str(line.trim());
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case(name) {
                out = Some(v.trim().to_string());
            } else if out.is_some() {
                break;
            }
        }
    }
    out
}

/// 从 From/To 头里取地址（`Name <a@b>` → `a@b`）
pub fn extract_addr(header_value: &str) -> String {
    if let (Some(start), Some(end)) = (header_value.rfind('<'), header_value.rfind('>')) {
        if start < end {
            return header_value[start + 1..end].trim().to_string();
        }
    }
    header_value.trim().to_string()
}

pub fn require<T>(opt: Option<T>, what: &str) -> Result<T> {
    opt.ok_or_else(|| anyhow!("缺少 {what}"))
}

#[cfg(test)]
mod utf7_tests {
    use super::decode_imap_utf7;

    #[test]
    fn decodes_qq_chinese_folders() {
        assert_eq!(decode_imap_utf7("&XfJT0ZAB-"), "已发送");
        assert_eq!(decode_imap_utf7("INBOX"), "INBOX");
        assert_eq!(decode_imap_utf7("Sent Messages"), "Sent Messages");
    }
}
