use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use mailctl::config::{self, Resolved};
use mailctl::imap::{extract_addr, header_unfolded, require, Imap};
use mailctl::mime::{self, MessageSpec};
use mailctl::smtp::Smtp;
use mailctl::{audit, cred};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "mailctl",
    version,
    about = "多邮箱 IMAP/SMTP 分发器：账号表驱动，带白名单 / 发前确认 / 审计"
)]
struct Cli {
    /// 账号表路径（默认 ~/.config/hermes-mail/accounts.toml，或 $MAILCTL_CONFIG）
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 输出能力清单（JSON，给 function calling / 自动化用）
    Capabilities {
        #[arg(long)]
        json: bool,
    },
    /// 列出所有账号（端点 + 凭证来源）
    Accounts {
        /// 机器可读输出（给 bot / 自动化用）
        #[arg(long)]
        json: bool,
    },
    /// 登录测试 IMAP + SMTP（不发信）
    Verify {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
    },
    /// 读取某封邮件正文（只读，多字符集解码；用于判断这封到底是什么）
    Read {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        uid: u32,
        #[arg(long, default_value_t = 2000)]
        chars: usize,
        /// 打印原始报文（诊断 MIME 用）
        #[arg(long)]
        raw: bool,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
    },
    /// 列出邮箱目录（含中文目录名解码）
    Folders {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
    },
    /// 只读扫描收件箱
    Scan {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
        /// 最多列多少封（0 = 全部）
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// 只看未读（UID SEARCH UNSEEN）
        #[arg(long)]
        unseen: bool,
        /// 目录：INBOX（默认）或「已发送」等，用于发后回读
        #[arg(long, default_value = "INBOX")]
        folder: String,
        /// 原始 IMAP 检索条件（如 'SUBJECT "mailctl"'），给就用它替代默认检索
        #[arg(long)]
        criteria: Option<String>,
        /// 大小写不敏感子串过滤（From+Subject），可用 | 分隔多个
        #[arg(long = "match")]
        pattern: Option<String>,
        /// 机器可读输出（JSON 数组）
        #[arg(long)]
        json: bool,
    },
    /// 新建邮件草稿（不发送）
    Draft {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        to: String,
        #[arg(long)]
        subject: String,
        #[arg(long = "body-file")]
        body_file: PathBuf,
        #[arg(long)]
        cc: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        /// 不追加该账号的签名
        #[arg(long)]
        no_signature: bool,
    },
    /// 按收件箱某封生成线程内回复草稿（不发送）
    Reply {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        uid: u32,
        #[arg(long = "body-file")]
        body_file: PathBuf,
        #[arg(long)]
        subject: Option<String>,
        #[arg(long)]
        cc: Option<String>,
        #[arg(long)]
        out: Option<PathBuf>,
        /// 不追加该账号的签名
        #[arg(long)]
        no_signature: bool,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
    },
    /// 自测闭环：用本账号给自己发一封，再到收件箱回读（不发给任何第三方）
    Selftest {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
        /// 发完等多少秒回读收件箱（0 = 不等）
        #[arg(long, default_value_t = 60)]
        wait: u64,
    },
    /// 真发 .eml（受 --yes 与白名单约束）
    Send {
        #[arg(short = 'a', long)]
        account: Option<String>,
        #[arg(long)]
        eml: PathBuf,
        /// 确认发送（confirm_before_send = true 时必需）
        #[arg(long)]
        yes: bool,
        #[arg(long = "append-sent")]
        append_sent: bool,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        prompt_password: bool,
    },
}

fn main() {
    if let Err(e) = run() {
        eprintln!("错误：{e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg_path = cli.config.unwrap_or_else(config::default_config_path);
    let cfg = config::load(&cfg_path)?;

    match cli.cmd {
        Cmd::Capabilities { json } => {
            let v: serde_json::Value = serde_json::from_str(mailctl::CAPABILITIES)?;
            if json {
                println!("{}", serde_json::to_string(&v)?);
            } else {
                println!("{}", serde_json::to_string_pretty(&v)?);
            }
            Ok(())
        }
        Cmd::Accounts { json } => cmd_accounts(&cfg, &cfg_path, json),
        Cmd::Read { account, uid, chars, raw, password, prompt_password } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_read(&acc, uid, chars, raw, password, prompt_password)
        }
        Cmd::Folders { account, password, prompt_password } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_folders(&acc, password, prompt_password)
        }
        Cmd::Verify { account, password, prompt_password } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_verify(&acc, password, prompt_password)
        }
        Cmd::Scan { account, password, prompt_password, limit, unseen, folder, criteria, pattern, json } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_scan(&acc, password, prompt_password, limit, unseen, &folder, criteria.as_deref(),
                     pattern.as_deref(), json)
        }
        Cmd::Draft { account, to, subject, body_file, cc, out, no_signature } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_draft(&acc, &to, &subject, &body_file, cc.as_deref(), out, no_signature)
        }
        Cmd::Reply { account, uid, body_file, subject, cc, out, no_signature, password, prompt_password } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_reply(&acc, uid, &body_file, subject.as_deref(), cc.as_deref(), out,
                      no_signature, password, prompt_password)
        }
        Cmd::Selftest { account, password, prompt_password, wait } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_selftest(&cfg, &acc, password, prompt_password, wait)
        }
        Cmd::Send { account, eml, yes, append_sent, password, prompt_password } => {
            let acc = config::resolve(&cfg, account.as_deref())?;
            cmd_send(&cfg, &acc, &eml, yes, append_sent, password, prompt_password)
        }
    }
}

fn cmd_accounts(cfg: &config::Config, path: &std::path::Path, json: bool) -> Result<()> {
    if cfg.accounts.is_empty() {
        bail!("账号表里没有 [accounts.*] 条目");
    }
    let default = cfg.default.account.clone();
    let confirm = cfg.default.confirm_before_send.unwrap_or(true);
    let mut rows = Vec::new();
    for name in cfg.accounts.keys() {
        let r = config::resolve(cfg, Some(name))?;
        let src = if let Some(s) = r.auth.keychain_service.as_deref().filter(|s| !s.is_empty()) {
            format!("keychain:{s}")
        } else if let Some(e) = r.auth.env.as_deref().filter(|s| !s.is_empty()) {
            format!("env:{e}")
        } else {
            "prompt".to_string()
        };
        rows.push(serde_json::json!({
            "name": r.name,
            "provider": r.provider,
            "address": r.address,
            "imap": format!("{}:{}({})", r.imap_host, r.imap_port, r.imap_security.label()),
            "smtp": format!("{}:{}({})", r.smtp_host, r.smtp_port, r.smtp_security.label()),
            "sent_folder": r.sent_folder,
            "credential": src,
            "source_file": r.origin,
            "signature": r.signature.as_deref().map(|s| s.lines().next().unwrap_or("").to_string()),
            "default": default.as_deref() == Some(name.as_str()),
            "allow_recipients": r.allow_recipients,
        }));
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("配置文件：{}", path.display());
    println!("默认账号：{}　确认发送：{confirm}", default.clone().unwrap_or_else(|| "（未设）".into()));
    println!("{}", "-".repeat(104));
    println!("{:<20}{:<20}{:<32}{:<28}{}", "名字", "provider", "地址", "IMAP", "凭证来源");
    for row in &rows {
        let name = row["name"].as_str().unwrap_or("");
        let mark = if row["default"].as_bool().unwrap_or(false) { " *" } else { "" };
        println!(
            "{:<20}{:<20}{:<32}{:<28}{}",
            format!("{name}{mark}"),
            row["provider"].as_str().unwrap_or(""),
            row["address"].as_str().unwrap_or(""),
            row["imap"].as_str().unwrap_or(""),
            row["credential"].as_str().unwrap_or(""),
        );
    }
    println!("{}", "-".repeat(104));
    println!("* = 默认账号。凭证不进账号表；发信受 allow_recipients 与 --yes 双重约束。");
    println!("账号 → 配置文件：");
    for row in &rows {
        println!(
            "  {:<20} {}{}",
            row["name"].as_str().unwrap_or(""),
            row["source_file"].as_str().unwrap_or("(未记录)"),
            match row["signature"].as_str() {
                Some(first) => format!("   签名首行：{first}"),
                None => "   签名：（无）".to_string(),
            }
        );
    }
    Ok(())
}

fn cmd_read(acc: &Resolved, uid: u32, chars: usize, raw_dump: bool,
            password: Option<String>, prompt: bool) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;
    let mut imap = Imap::connect(acc)?;
    imap.login(&acc.address, &pw)?;
    imap.examine("INBOX")?;
    let raw = imap.fetch_full(uid)?;
    imap.logout();
    if raw_dump {
        println!("原始字节数：{}", raw.len());
        let head: String = String::from_utf8_lossy(&raw).chars().take(chars).collect();
        println!("{head}");
        return Ok(());
    }
    let from = header_unfolded(&raw, "From").unwrap_or_default();
    let date = header_unfolded(&raw, "Date").unwrap_or_default();
    let subject = mime::decode_rfc2047(&header_unfolded(&raw, "Subject").unwrap_or_default());
    let text = mime::extract_text(&raw);
    println!("UID {uid}");
    println!("  From: {from}");
    println!("  Date: {date}");
    println!("  Subj: {subject}");
    println!("{}", "-".repeat(70));
    let total = text.chars().count();
    let shown: String = text.chars().take(chars).collect();
    println!("{shown}");
    if total > chars {
        println!("……（已截断，正文共 {total} 字）");
    }
    Ok(())
}

fn cmd_folders(acc: &Resolved, password: Option<String>, prompt: bool) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;
    let mut imap = Imap::connect(acc)?;
    imap.login(&acc.address, &pw)?;
    let folders = imap.list_folders()?;
    imap.logout();
    println!("[{}] 目录 {} 个：", acc.name, folders.len());
    for (raw, readable) in folders {
        if raw == readable {
            println!("  {raw}");
        } else {
            println!("  {readable}    ({raw})");
        }
    }
    Ok(())
}

fn cmd_verify(acc: &Resolved, password: Option<String>, prompt: bool) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;
    println!("[{}] {}  provider={}", acc.name, acc.address, acc.provider);
    let mut rc = 0;
    match Imap::connect(acc).and_then(|mut i| {
        i.login(&acc.address, &pw)?;
        let n = i.examine_inbox()?;
        i.logout();
        Ok(n)
    }) {
        Ok(n) => println!("  IMAP  {}:{} 登录 OK，INBOX 只读打开（{n} 封）", acc.imap_host, acc.imap_port),
        Err(e) => {
            println!("  IMAP  FAIL：{e:#}");
            rc = 1;
        }
    }
    match Smtp::connect(acc).and_then(|mut s| {
        s.ehlo()?;
        s.auth(&acc.address, &pw)?;
        s.quit();
        Ok(())
    }) {
        Ok(()) => println!("  SMTP  {}:{} 登录 OK（未发送任何邮件）", acc.smtp_host, acc.smtp_port),
        Err(e) => {
            println!("  SMTP  FAIL：{e:#}");
            rc = 1;
        }
    }
    if rc != 0 {
        bail!("verify 有失败项");
    }
    Ok(())
}

fn match_any(blob: &str, pattern: &str) -> bool {
    let low = blob.to_lowercase();
    pattern
        .split('|')
        .map(|p| p.trim().to_lowercase())
        .filter(|p| !p.is_empty())
        .any(|p| low.contains(&p))
}

#[allow(clippy::too_many_arguments)]
fn cmd_scan(acc: &Resolved, password: Option<String>, prompt: bool, limit: usize, unseen: bool,
            folder: &str, criteria: Option<&str>, pattern: Option<&str>, json: bool) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;
    let mut imap = Imap::connect(acc)?;
    imap.login(&acc.address, &pw)?;
    let exists = imap.examine(folder)?;
    let criteria = match criteria {
        Some(c) => c.to_string(),
        None if unseen => "UNSEEN".to_string(),
        None => "ALL".to_string(),
    };
    let mut uids = imap.uid_search(&criteria)?;
    if limit > 0 && uids.len() > limit {
        uids = uids.split_off(uids.len() - limit);
    }
    if !json {
        println!(
            "[{}] {} 共 {exists} 封，检查最近 {} 封（只读，不取正文、不标已读）{}",
            acc.name,
            folder,
            uids.len(),
            if unseen { "，仅未读" } else { "" }
        );
    }
    let mut rows = Vec::new();
    for uid in uids.iter().rev() {
        let raw = imap.fetch_header(*uid)?;
        let from = header_unfolded(&raw, "From").unwrap_or_default();
        let subject = mime::decode_rfc2047(&header_unfolded(&raw, "Subject").unwrap_or_default());
        let blob = format!("{from} {subject}");
        if let Some(p) = pattern {
            if !match_any(&blob, p) {
                continue;
            }
        }
        let row = serde_json::json!({
            "uid": uid,
            "folder": folder,
            "date": header_unfolded(&raw, "Date").unwrap_or_default(),
            "from": from,
            "subject": subject,
            "message_id": header_unfolded(&raw, "Message-ID").unwrap_or_default(),
            "references": header_unfolded(&raw, "References").unwrap_or_default(),
        });
        if !json {
            println!("{}", "-".repeat(78));
            println!("UID {}  |  {}", row["uid"], row["date"].as_str().unwrap_or(""));
            println!("  From: {}", row["from"].as_str().unwrap_or(""));
            println!("  Subj: {}", row["subject"].as_str().unwrap_or(""));
            println!("  MsgID: {}", row["message_id"].as_str().unwrap_or(""));
            if !row["references"].as_str().unwrap_or("").is_empty() {
                println!("  Refs: {}", row["references"].as_str().unwrap_or(""));
            }
        }
        rows.push(row);
    }
    imap.logout();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    println!("{}", "-".repeat(78));
    println!(
        "命中 {} 封。{}",
        rows.len(),
        if pattern.is_some() { "" } else { "（未加 --match，全部列出）" }
    );
    Ok(())
}

/// 追加账号签名（幂等：正文里已有就不重复加）
fn apply_signature(body: &str, sig: Option<&str>, skip: bool) -> String {
    if skip {
        return body.to_string();
    }
    let Some(sig) = sig.map(str::trim).filter(|s| !s.is_empty()) else {
        return body.to_string();
    };
    if body.contains(sig) {
        return body.to_string();
    }
    format!("{}\n\n{sig}\n", body.trim_end())
}

fn cmd_draft(acc: &Resolved, to: &str, subject: &str, body_file: &std::path::Path,
             cc: Option<&str>, out: Option<PathBuf>, no_signature: bool) -> Result<()> {
    let raw_body = std::fs::read_to_string(body_file)
        .with_context(|| format!("读不到正文 {}", body_file.display()))?;
    let body = apply_signature(&raw_body, acc.signature.as_deref(), no_signature);
    let spec = MessageSpec {
        from_name: acc.display_name.as_deref(),
        from_addr: &acc.address,
        to,
        cc,
        subject,
        body: &body,
        in_reply_to: None,
        references: None,
    };
    let built = mime::build(&spec);
    let path = out.unwrap_or_else(|| PathBuf::from(format!(
        "draft-{}-{}.eml",
        acc.name,
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    )));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, &built.raw)?;
    println!("草稿（{}）：{}", acc.name, path.display());
    println!("  From: {} <{}>", acc.display_name.clone().unwrap_or_default(), acc.address);
    println!("  To: {to}");
    println!("  Subject: {subject}");
    println!("  Message-ID: {}", built.message_id);
    println!("未发送。确认后：mailctl send -a {} --eml {} --yes", acc.name, path.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn cmd_reply(acc: &Resolved, uid: u32, body_file: &std::path::Path, subject: Option<&str>,
             cc: Option<&str>, out: Option<PathBuf>, no_signature: bool,
             password: Option<String>, prompt: bool) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;
    let mut imap = Imap::connect(acc)?;
    imap.login(&acc.address, &pw)?;
    imap.examine_inbox()?;
    let raw = imap.fetch_header(uid)?;
    imap.logout();

    let from_hdr = require(header_unfolded(&raw, "From"), "原邮件 From")?;
    let to = mime::format_address(&from_hdr);
    let orig_subject = mime::decode_rfc2047(&header_unfolded(&raw, "Subject").unwrap_or_default());
    let subject = match subject {
        Some(s) => s.to_string(),
        None if orig_subject.to_lowercase().starts_with("re:") => orig_subject.clone(),
        None => format!("Re: {orig_subject}"),
    };
    let in_reply_to = header_unfolded(&raw, "Message-ID");
    let references = header_unfolded(&raw, "References");
    let raw_body = std::fs::read_to_string(body_file)
        .with_context(|| format!("读不到正文 {}", body_file.display()))?;
    let body = apply_signature(&raw_body, acc.signature.as_deref(), no_signature);

    let spec = MessageSpec {
        from_name: acc.display_name.as_deref(),
        from_addr: &acc.address,
        to: &to,
        cc,
        subject: &subject,
        body: &body,
        in_reply_to: in_reply_to.as_deref(),
        references: references.as_deref(),
    };
    let built = mime::build(&spec);
    let path = out.unwrap_or_else(|| PathBuf::from(format!("reply-{}-{uid}.eml", acc.name)));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    std::fs::write(&path, &built.raw)?;
    println!("草稿（{}）：{}", acc.name, path.display());
    println!("  To: {to}");
    println!("  Subject: {subject}");
    println!("  In-Reply-To: {}", in_reply_to.unwrap_or_default());
    println!("未发送。");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_send(cfg: &config::Config, acc: &Resolved, eml: &std::path::Path, yes: bool,
            append_sent: bool, password: Option<String>, prompt: bool) -> Result<()> {
    let raw = std::fs::read(eml).with_context(|| format!("读不到 {}", eml.display()))?;
    let to_hdr = header_unfolded(&raw, "To").unwrap_or_default();
    let cc_hdr = header_unfolded(&raw, "Cc").unwrap_or_default();
    let subject = mime::decode_rfc2047(&header_unfolded(&raw, "Subject").unwrap_or_default());
    let from_hdr = header_unfolded(&raw, "From").unwrap_or_default();
    let from_addr = extract_addr(&from_hdr);
    let message_id = header_unfolded(&raw, "Message-ID").unwrap_or_default();
    let in_reply_to = header_unfolded(&raw, "In-Reply-To").unwrap_or_default();

    let confirm = cfg.default.confirm_before_send.unwrap_or(true);
    if confirm && !yes {
        eprintln!("拒发：「{}」配置了 confirm_before_send=true。确认内容无误后加 --yes。", acc.name);
        eprintln!("  收件人：{to_hdr}\n  主题：{subject}");
        std::process::exit(3);
    }

    let mut rcpts = mime::parse_recipients(&to_hdr);
    rcpts.extend(mime::parse_recipients(&cc_hdr));
    acc.guard(&rcpts)?;

    // 发件地址必须与本账号一致，避免"用 A 身份发 B 的信"
    if !from_addr.is_empty() && !from_addr.eq_ignore_ascii_case(&acc.address) {
        bail!(
            "身份不符：.eml 的 From={from_addr}，账号「{}」是 {}。拒绝用错身份发信。",
            acc.name,
            acc.address
        );
    }

    let pw = cred::password(acc, password, prompt)?;
    println!("[{}] 发送中 → {to_hdr} ｜ {subject}", acc.name);
    let mut smtp = Smtp::connect(acc)?;
    smtp.ehlo()?;
    smtp.auth(&acc.address, &pw)?;
    smtp.send(&acc.address, &rcpts, &raw)?;
    smtp.quit();
    println!("SMTP 已接受（250）。");

    let audit_path = audit_path_for(cfg);
    audit::sent(&audit_path, &acc.name, &acc.address, &to_hdr, &cc_hdr, &subject,
                &message_id, &in_reply_to, &acc.smtp_host)?;
    println!("审计已记录：{}", audit_path.display());

    if append_sent {
        let res = Imap::connect(acc).and_then(|mut i| {
            i.login(&acc.address, &pw)?;
            let r = i.append(&acc.sent_folder, &raw)?;
            i.logout();
            Ok(r)
        });
        match res {
            Ok(r) => println!("已 APPEND 到「{}」：{}", acc.sent_folder, r.trim()),
            Err(e) => eprintln!("警告：APPEND 失败（邮件已发出，勿重发）：{e:#}"),
        }
    }
    Ok(())
}

/// 审计路径：绝对路径原样用；相对路径按 $HOME 解析（绝不跟随 cwd，免得审计落到任意目录）
fn audit_path_for(cfg: &config::Config) -> PathBuf {
    match cfg.default.audit_log.as_deref() {
        Some(s) if !s.trim().is_empty() => {
            let p = PathBuf::from(shellexpand(s));
            if p.is_absolute() {
                p
            } else {
                config::home().join(p)
            }
        }
        _ => config::default_audit_path(),
    }
}

/// 闭环自测：IMAP 登录 → 给自己发一封 → 回读 INBOX 确认。全程不外发第三方。
fn cmd_selftest(cfg: &config::Config, acc: &Resolved, password: Option<String>,
                prompt: bool, wait: u64) -> Result<()> {
    let pw = cred::password(acc, password, prompt)?;

    // 1) IMAP 读路径
    let mut imap = Imap::connect(acc)?;
    imap.login(&acc.address, &pw)?;
    let exists = imap.examine("INBOX")?;
    imap.logout();
    println!("[{}] IMAP 只读打开 OK（INBOX {exists} 封）", acc.name);

    // 2) 组一封给自己的信
    let stamp = audit::now();
    // 唯一标签放在主题里：QQ 等厂商会改写 Message-ID，但主题不会被改
    let tag = format!("mailctl-selftest-{}", stamp.replace([':', '-', '.'], "").replace('+', "T"));
    let subject = format!("[mailctl selftest] {tag}");
    let body = format!(
        "这是一封 mailctl 自测邮件，只发给你自己的邮箱。\n\n账号：{}\n时间：{}\nSMTP：{}\n\n收到即说明「发信 + 收信」双向都通。\n",
        acc.address, stamp, acc.smtp_host
    );
    let built = mime::build(&MessageSpec {
        from_name: acc.display_name.as_deref(),
        from_addr: &acc.address,
        to: &acc.address,
        cc: None,
        subject: &subject,
        body: &body,
        in_reply_to: None,
        references: None,
    });
    let rcpts = vec![acc.address.clone()];
    acc.guard(&rcpts)?; // 自发自收：白名单对 self 自动放行

    // 3) SMTP 发信
    let mut smtp = Smtp::connect(acc)?;
    smtp.ehlo()?;
    smtp.auth(&acc.address, &pw)?;
    smtp.send(&acc.address, &rcpts, &built.raw)?;
    smtp.quit();
    println!("[{}] SMTP 已接受（250）｜ Message-ID {}", acc.name, built.message_id);
    println!("  主题：{subject}");

    // 4) 回读收件箱
    let mut delivered = false;
    let mut matched_uid: Option<u32> = None;
    let mut rewritten_id: Option<String> = None;
    let mut waited = 0u64;
    if wait > 0 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_secs(10));
            waited += 10;
            if let Ok(mut i) = Imap::connect(acc) {
                let ok = i.login(&acc.address, &pw).is_ok() && i.examine("INBOX").is_ok();
                if ok {
                    // 按主题标签精确检索（主题不会被厂商改写）
                    let crit = format!("SUBJECT \"{tag}\"");
                    if let Ok(uids) = i.uid_search(&crit) {
                        for uid in uids.iter().rev().take(5) {
                            if let Ok(raw) = i.fetch_header(*uid) {
                                let subj = mime::decode_rfc2047(
                                    &header_unfolded(&raw, "Subject").unwrap_or_default(),
                                );
                                if subj.contains(&tag) {
                                    delivered = true;
                                    matched_uid = Some(*uid);
                                    let mid = header_unfolded(&raw, "Message-ID").unwrap_or_default();
                                    if mid.trim() != built.message_id {
                                        rewritten_id = Some(mid.trim().to_string());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                i.logout();
            }
            if delivered {
                break;
            }
            println!("  … 已等 {waited}s，继续回读");
        }
    }

    audit::selftest(
        &audit_path_for(cfg),
        &acc.name,
        &acc.address,
        &acc.address,
        &subject,
        &built.message_id,
        &acc.smtp_host,
        delivered,
    )?;

    if delivered {
        println!(
            "回读成功：INBOX 已看到这封自测信（{}s 内，UID {}）。闭环通。",
            waited,
            matched_uid.map(|u| u.to_string()).unwrap_or_else(|| "?".into())
        );
        if let Some(mid) = &rewritten_id {
            println!("  注意：厂商改写了 Message-ID → {mid}（线程仍靠 References 维持，不影响回复）");
        }
    } else if wait == 0 {
        println!("已发送；--wait 0 跳过回读。");
    } else {
        eprintln!("警告：{wait}s 内未在 INBOX 看到回信（可能延迟/进垃圾箱）；SMTP 侧已接受。");
    }
    Ok(())
}

fn shellexpand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        format!("{}/{}", config::home().display(), rest)
    } else {
        p.to_string()
    }
}
