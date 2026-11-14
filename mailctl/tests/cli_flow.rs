//! 端到端：起假 IMAP + 假 SMTP，直接跑编译出来的 mailctl 二进制。
//! 覆盖：scan / reply / send 拒发(--yes 闸) / send 成功 / 白名单拦截 / 审计落盘。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

const IMAP_101: &str = "From: Anny <developer@apple.com>\r\nTo: 416646744@qq.com\r\nSubject: =?utf-8?B?5YWz5LqO5rOo5YaM55Sz6K+3IFpZOEhVTDY3RDM=?=\r\nDate: Fri, 13 Nov 2026 09:00:00 +0800\r\nMessage-ID: <CASE-30000006887467@apple.com>\r\nReferences: <CASE-ROOT-30000006887467@apple.com>\r\n\r\n";
const IMAP_102: &str = "From: Tencent Cloud <noreply@tencent.com>\r\nTo: 416646744@qq.com\r\nSubject: 续费提醒\r\nDate: Fri, 13 Nov 2026 10:00:00 +0800\r\nMessage-ID: <tc-102@tencent.com>\r\n\r\n";

fn spawn_imap() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind imap");
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            thread::spawn(move || serve_imap(stream));
        }
    });
    port
}

fn serve_imap(mut s: TcpStream) {
    let mut writer = s.try_clone().unwrap();
    let mut reader = BufReader::new(s.try_clone().unwrap());
    let _ = writer.write_all(b"* OK mock imap ready\r\n");
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let parts: Vec<&str> = line.trim_end().split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let tag = parts[0];
        let verb = parts[1].to_ascii_uppercase();
        match verb.as_str() {
            "CAPABILITY" => {
                let _ = writer.write_all(format!("* CAPABILITY IMAP4rev1\r\n{tag} OK done\r\n").as_bytes());
            }
            "LOGIN" => {
                let _ = writer.write_all(format!("{tag} OK login done\r\n").as_bytes());
            }
            "EXAMINE" | "SELECT" => {
                let _ = writer.write_all(format!("* 2 EXISTS\r\n{tag} OK [READ-ONLY] done\r\n").as_bytes());
            }
            "UID" => {
                let sub = parts.get(2).map(|s| s.to_ascii_uppercase()).unwrap_or_default();
                if sub == "SEARCH" {
                    let _ = writer.write_all(format!("* SEARCH 101 102\r\n{tag} OK done\r\n").as_bytes());
                } else if sub == "FETCH" {
                    let uid = parts.get(3).copied().unwrap_or("101");
                    let body = if uid.contains("101") { IMAP_101 } else { IMAP_102 };
                    let _ = writer.write_all(
                        format!("* 1 FETCH (UID {uid} BODY[HEADER] {{{}}}\r\n", body.len()).as_bytes(),
                    );
                    let _ = writer.write_all(body.as_bytes());
                    let _ = writer.write_all(b")\r\n");
                    let _ = writer.write_all(format!("{tag} OK done\r\n").as_bytes());
                } else {
                    let _ = writer.write_all(format!("{tag} BAD uid sub\r\n").as_bytes());
                }
            }
            "APPEND" => {
                let size = line
                    .trim_end()
                    .rsplit('{')
                    .next()
                    .and_then(|s| s.trim_end_matches('}').parse::<usize>().ok())
                    .unwrap_or(0);
                let _ = writer.write_all(b"+ Ready\r\n");
                let mut buf = vec![0u8; size];
                if reader.read_exact(&mut buf).is_err() {
                    return;
                }
                let mut crlf = [0u8; 2];
                let _ = reader.read_exact(&mut crlf);
                let _ = writer.write_all(format!("{tag} OK append done\r\n").as_bytes());
            }
            "LOGOUT" => {
                let _ = writer.write_all(format!("* BYE\r\n{tag} OK bye\r\n").as_bytes());
                return;
            }
            _ => {
                let _ = writer.write_all(format!("{tag} BAD unsupported\r\n").as_bytes());
            }
        }
    }
}

fn spawn_smtp() -> (u16, Arc<Mutex<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind smtp");
    let port = listener.local_addr().unwrap().port();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let cap = captured.clone();
    thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(stream) = conn else { continue };
            let cap = cap.clone();
            thread::spawn(move || serve_smtp(stream, cap));
        }
    });
    (port, captured)
}

fn serve_smtp(mut s: TcpStream, cap: Arc<Mutex<Vec<u8>>>) {
    let mut writer = s.try_clone().unwrap();
    let mut reader = BufReader::new(s.try_clone().unwrap());
    let _ = writer.write_all(b"220 mock smtp\r\n");
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let verb = line.split_whitespace().next().unwrap_or("").to_ascii_uppercase();
        match verb.as_str() {
            "EHLO" => {
                let _ = writer.write_all(b"250-mock\r\n250-AUTH PLAIN LOGIN\r\n250 OK\r\n");
            }
            "HELO" => {
                let _ = writer.write_all(b"250 mock\r\n");
            }
            "AUTH" => {
                let _ = writer.write_all(b"235 2.7.0 ok\r\n");
            }
            "MAIL" | "RCPT" => {
                let _ = writer.write_all(b"250 ok\r\n");
            }
            "DATA" => {
                let _ = writer.write_all(b"354 go ahead\r\n");
                let mut data = Vec::new();
                loop {
                    let mut buf = Vec::new();
                    if reader.read_until(b'\n', &mut buf).unwrap_or(0) == 0 {
                        return;
                    }
                    if buf == b".\r\n" {
                        break;
                    }
                    data.extend_from_slice(&buf);
                }
                *cap.lock().unwrap() = data;
                let _ = writer.write_all(b"250 queued\r\n");
            }
            "QUIT" => {
                let _ = writer.write_all(b"221 bye\r\n");
                return;
            }
            _ => {
                let _ = writer.write_all(b"250 ok\r\n");
            }
        }
    }
}

fn tmpdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mailctl-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_config(dir: &Path, imap_port: u16, smtp_port: u16, allow: &str) -> PathBuf {
    let cfg = format!(
        r#"
[default]
account = "mock"
confirm_before_send = true
audit_log = "{audit}"

[accounts.mock]
provider = "custom"
address = "416646744@qq.com"
display_name = "段文彬 | Mujin Technology"
imap_host = "127.0.0.1"
imap_port = {imap_port}
imap_security = "plain"
smtp_host = "127.0.0.1"
smtp_port = {smtp_port}
smtp_security = "plain"
sent_folder = "Sent Messages"
allow_recipients = [{allow}]
"#,
        audit = dir.join("audit.log").display(),
        allow = allow
    );
    let path = dir.join("accounts.toml");
    std::fs::write(&path, cfg).unwrap();
    path
}

fn run(cfg: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_mailctl"))
        .arg("--config")
        .arg(cfg)
        .args(args)
        .env("MAILCTL_AUDIT", cfg.with_file_name("audit.log"))
        .output()
        .expect("run mailctl");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn full_cli_flow_against_mock_servers() {
    let imap_port = spawn_imap();
    let (smtp_port, captured) = spawn_smtp();
    let dir = tmpdir("flow");
    let cfg = write_config(&dir, imap_port, smtp_port, "\"apple.com\"");
    let body = dir.join("body.txt");
    std::fs::write(&body, "Anny 你好：\n\n材料已上传完毕。\n\n谨启\n段文彬\n").unwrap();

    // 1) scan，只留 Apple 那封
    let (code, out, err) = run(
        &cfg,
        &["scan", "-a", "mock", "--limit", "5", "--match", "apple|ZY8HUL67D3", "--password", "dummy"],
    );
    assert_eq!(code, 0, "scan failed: {err}");
    assert!(out.contains("Anny <developer@apple.com>"), "stdout: {out}");
    assert!(out.contains("命中 1 封"), "stdout: {out}");

    // 2) reply：线程内草稿
    let eml = dir.join("reply.eml");
    let (code, out, err) = run(
        &cfg,
        &["reply", "-a", "mock", "--uid", "101", "--body-file", body.to_str().unwrap(),
          "--out", eml.to_str().unwrap(), "--password", "dummy"],
    );
    assert_eq!(code, 0, "reply failed: {err}");
    assert!(out.contains("In-Reply-To: <CASE-30000006887467@apple.com>"), "stdout: {out}");
    let raw = std::fs::read_to_string(&eml).unwrap();
    assert!(raw.contains("References: <CASE-ROOT-30000006887467@apple.com> <CASE-30000006887467@apple.com>"));

    // 3) 不带 --yes：必须拒发，exit 3，且 SMTP 没收到任何东西
    let (code, _, err) = run(&cfg, &["send", "-a", "mock", "--eml", eml.to_str().unwrap(), "--password", "dummy"]);
    assert_eq!(code, 3, "缺少 --yes 应拒发，stderr: {err}");
    assert!(err.contains("confirm_before_send"), "stderr: {err}");
    assert!(captured.lock().unwrap().is_empty());

    // 4) 带 --yes：真发到假 SMTP
    let (code, out, err) = run(
        &cfg,
        &["send", "-a", "mock", "--eml", eml.to_str().unwrap(), "--password", "dummy", "--yes"],
    );
    assert_eq!(code, 0, "send failed: {err}\nstdout: {out}");
    let mail = String::from_utf8_lossy(&captured.lock().unwrap()).to_string();
    assert!(mail.contains("To: Anny <developer@apple.com>"), "mail: {mail}");
    assert!(mail.contains("In-Reply-To: <CASE-30000006887467@apple.com>"));

    // 5) 审计落盘
    let audit = std::fs::read_to_string(dir.join("audit.log")).unwrap();
    assert!(audit.contains("\"kind\":\"sent\""), "audit: {audit}");
    assert!(audit.contains("\"account\":\"mock\""));
}

#[test]
fn selftest_sends_to_self_and_audits() {
    let imap_port = spawn_imap();
    let (smtp_port, captured) = spawn_smtp();
    let dir = tmpdir("selftest");
    // 白名单只允许 apple.com，但自测发给自己必须放行
    let cfg = write_config(&dir, imap_port, smtp_port, "\"apple.com\"");

    let (code, out, err) = run(
        &cfg,
        &["selftest", "-a", "mock", "--password", "dummy", "--wait", "0"],
    );
    assert_eq!(code, 0, "selftest failed: {err}\nstdout: {out}");
    assert!(out.contains("IMAP 只读打开 OK"), "stdout: {out}");
    assert!(out.contains("SMTP 已接受"), "stdout: {out}");

    let mail = String::from_utf8_lossy(&captured.lock().unwrap()).to_string();
    assert!(mail.contains("To: 416646744@qq.com"), "mail: {mail}");
    assert!(mail.contains("[mailctl selftest]"), "mail: {mail}");

    let audit = std::fs::read_to_string(dir.join("audit.log")).unwrap();
    assert!(audit.contains("\"kind\":\"selftest\""), "audit: {audit}");
}

#[test]
fn allowlist_blocks_foreign_recipient() {
    let imap_port = spawn_imap();
    let (smtp_port, captured) = spawn_smtp();
    let dir = tmpdir("guard");
    let cfg = write_config(&dir, imap_port, smtp_port, "\"apple.com\"");
    let body = dir.join("body.txt");
    std::fs::write(&body, "hi").unwrap();

    let eml = dir.join("to-other.eml");
    let (code, _, err) = run(
        &cfg,
        &["draft", "-a", "mock", "--to", "someone@tencent.com", "--subject", "x",
          "--body-file", body.to_str().unwrap(), "--out", eml.to_str().unwrap()],
    );
    assert_eq!(code, 0, "draft failed: {err}");

    let (code, _, err) = run(
        &cfg,
        &["send", "-a", "mock", "--eml", eml.to_str().unwrap(), "--password", "dummy", "--yes"],
    );
    assert_ne!(code, 0, "白名单应拦截");
    assert!(err.contains("白名单拦截"), "stderr: {err}");
    assert!(captured.lock().unwrap().is_empty());
}

#[test]
fn identity_mismatch_is_refused() {
    let imap_port = spawn_imap();
    let (smtp_port, _cap) = spawn_smtp();
    let dir = tmpdir("identity");
    let cfg = write_config(&dir, imap_port, smtp_port, "\"apple.com\"");
    // 手工造一封 From 不是本账号的 .eml
    let eml = dir.join("foreign.eml");
    std::fs::write(
        &eml,
        "From: 别人 <other@elsewhere.com>\r\nTo: a@apple.com\r\nSubject: x\r\nMessage-ID: <1@x>\r\n\r\nbody\r\n",
    )
    .unwrap();
    let (code, _, err) = run(
        &cfg,
        &["send", "-a", "mock", "--eml", eml.to_str().unwrap(), "--password", "dummy", "--yes"],
    );
    assert_ne!(code, 0);
    assert!(err.contains("身份不符"), "stderr: {err}");
}

#[test]
fn scan_json_and_folder_are_usable_by_bots() {
    let imap_port = spawn_imap();
    let (smtp_port, _cap) = spawn_smtp();
    let dir = tmpdir("json");
    let cfg = write_config(&dir, imap_port, smtp_port, "\"apple.com\"");

    let (code, out, err) = run(
        &cfg,
        &["scan", "-a", "mock", "--limit", "5", "--match", "apple",
          "--folder", "Sent Messages", "--json", "--password", "dummy"],
    );
    assert_eq!(code, 0, "scan --json failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect("输出应是合法 JSON");
    let arr = v.as_array().expect("应是数组");
    assert_eq!(arr.len(), 1, "应只命中 1 封: {out}");
    assert_eq!(arr[0]["message_id"], "<CASE-30000006887467@apple.com>");
    assert_eq!(arr[0]["folder"], "Sent Messages");
    assert!(arr[0]["subject"].as_str().unwrap().contains("ZY8HUL67D3"));
}

#[test]
fn accounts_json_is_machine_readable() {
    let dir = tmpdir("accountsjson");
    let cfg_path = dir.join("accounts.toml");
    std::fs::write(
        &cfg_path,
        r#"
[default]
account = "work"
[accounts.work]
provider = "exmail"
address = "a@corp.com"
auth.keychain_service = "mail-work"
allow_recipients = ["corp.com"]
"#,
    )
    .unwrap();
    let (code, out, err) = run(&cfg_path, &["accounts", "--json"]);
    assert_eq!(code, 0, "accounts --json failed: {err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["name"], "work");
    assert_eq!(v[0]["imap"], "imap.exmail.qq.com:993(tls)");
    assert_eq!(v[0]["credential"], "keychain:mail-work");
    assert_eq!(v[0]["default"], true);
}

#[test]
fn accounts_lists_presets_without_network() {
    let dir = tmpdir("accounts");
    let cfg_path = dir.join("accounts.toml");
    std::fs::write(
        &cfg_path,
        r#"
[default]
account = "work"
[accounts.work]
provider = "exmail"
address = "a@corp.com"
auth.keychain_service = "mail-work"
[accounts.ops]
provider = "aliyun-enterprise"
address = "ops@corp.com"
auth.env = "ALI_PW"
"#,
    )
    .unwrap();
    let (code, out, err) = run(&cfg_path, &["accounts"]);
    assert_eq!(code, 0, "accounts failed: {err}");
    assert!(out.contains("imap.exmail.qq.com:993(tls)"), "stdout: {out}");
    assert!(out.contains("imap.mxhichina.com:993(tls)"), "stdout: {out}");
    assert!(out.contains("keychain:mail-work"));
    assert!(out.contains("env:ALI_PW"));
}
