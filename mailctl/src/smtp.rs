//! 极简 SMTP 客户端：EHLO / AUTH PLAIN|LOGIN / MAIL / RCPT / DATA(dot-stuffing) / QUIT。

use crate::config::{Resolved, Security};
use crate::net::{connect, Conn};
use anyhow::{bail, Result};
use base64::Engine;

pub struct Smtp {
    conn: Conn,
}

impl Smtp {
    pub fn connect(r: &Resolved) -> Result<Smtp> {
        let stream = connect(&r.smtp_host, r.smtp_port, r.smtp_security)?;
        let mut conn = Conn::new(stream);
        let hello = conn.read_line()?;
        if !hello.starts_with("220") {
            bail!("SMTP 问候异常：{hello}");
        }
        let mut smtp = Smtp { conn };
        if r.smtp_security == Security::StartTls {
            let resp = smtp.expect_cmd("STARTTLS", "220")?;
            if !resp.starts_with("220") {
                bail!("STARTTLS 被拒：{resp}");
            }
            smtp.conn.upgrade_tls(&r.smtp_host)?;
        }
        Ok(smtp)
    }

    fn read_reply(&mut self) -> Result<String> {
        let mut acc = String::new();
        loop {
            let line = self.conn.read_line()?;
            let cont = line.get(3..4) == Some("-");
            acc.push_str(&line);
            if !cont {
                acc.push('\n');
                return Ok(acc);
            }
            acc.push('\n');
        }
    }

    fn cmd(&mut self, text: &str) -> Result<String> {
        self.conn.write_line(text)?;
        self.read_reply()
    }

    fn expect_cmd(&mut self, text: &str, code: &str) -> Result<String> {
        let resp = self.cmd(text)?;
        if !resp.starts_with(code) {
            bail!("SMTP 命令 {text} 失败（期望 {code}）：{resp}");
        }
        Ok(resp)
    }

    pub fn ehlo(&mut self) -> Result<String> {
        match self.cmd("EHLO mailctl.local") {
            Ok(r) if r.starts_with("250") => Ok(r),
            _ => self.expect_cmd("HELO mailctl.local", "250"),
        }
    }

    pub fn auth(&mut self, user: &str, pass: &str) -> Result<()> {
        let b64 = base64::engine::general_purpose::STANDARD;
        let plain = b64.encode(format!("\0{user}\0{pass}"));
        let resp = self.cmd(&format!("AUTH PLAIN {plain}"))?;
        if resp.starts_with("235") {
            return Ok(());
        }
        // 回退 AUTH LOGIN
        let r1 = self.cmd("AUTH LOGIN")?;
        if !r1.starts_with("334") {
            bail!("SMTP AUTH 不支持或失败：{resp} / {r1}");
        }
        self.expect_cmd(&b64.encode(user), "334")?;
        let r3 = self.cmd(&b64.encode(pass))?;
        if !r3.starts_with("235") {
            bail!("SMTP 认证失败（授权码/应用密码不对？）：{r3}");
        }
        Ok(())
    }

    pub fn send(&mut self, from: &str, rcpts: &[String], data: &[u8]) -> Result<String> {
        self.expect_cmd(&format!("MAIL FROM:<{from}>"), "250")?;
        for r in rcpts {
            self.expect_cmd(&format!("RCPT TO:<{r}>"), "250")?;
        }
        self.expect_cmd("DATA", "354")?;
        let stuffed = dot_stuff(data);
        self.conn.write_raw(&stuffed)?;
        self.conn.write_raw(b"\r\n.\r\n")?;
        let resp = self.read_reply()?;
        if !resp.starts_with("250") {
            bail!("DATA 被拒：{resp}");
        }
        Ok(resp)
    }

    pub fn quit(&mut self) {
        let _ = self.cmd("QUIT");
    }
}

/// RFC 5321 点填充：行首的 '.' 变成 '..'
fn dot_stuff(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut at_line_start = true;
    for &b in data {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    out
}

pub fn requires_starttls(sec: Security) -> bool {
    sec == Security::StartTls
}

/// 给测试用的点填充入口
pub fn dot_stuff_pub(data: &[u8]) -> Vec<u8> {
    dot_stuff(data)
}
