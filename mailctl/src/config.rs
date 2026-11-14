//! 账号表：TOML 解析、服务商预设、账号解析、收件人白名单。

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub default: Defaults,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Defaults {
    pub account: Option<String>,
    pub confirm_before_send: Option<bool>,
    pub audit_log: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Auth {
    pub keychain_service: Option<String>,
    pub env: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct Account {
    pub provider: Option<String>,
    pub address: String,
    pub display_name: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub imap_security: Option<String>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_security: Option<String>,
    pub sent_folder: Option<String>,
    #[serde(default)]
    pub allow_recipients: Vec<String>,
    pub auth: Option<Auth>,
    /// 邮件签名（多行）；也可以用 signature_file 指向同目录下的文本
    pub signature: Option<String>,
    pub signature_file: Option<String>,
    /// 该账号来自哪个配置文件（目录模式自动填，方便人和 AI 知道该改哪个文件）
    #[serde(skip)]
    pub origin: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Tls,
    StartTls,
    Plain,
}

impl Security {
    fn parse(s: &str, what: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tls" | "ssl" => Ok(Security::Tls),
            "starttls" => Ok(Security::StartTls),
            "plain" | "none" => Ok(Security::Plain),
            other => bail!("{what} 的 security={other:?} 不认识（tls|starttls|plain）"),
        }
    }
    pub fn label(&self) -> &'static str {
        match self {
            Security::Tls => "tls",
            Security::StartTls => "starttls",
            Security::Plain => "plain",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub address: String,
    pub display_name: Option<String>,
    pub provider: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: Security,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: Security,
    pub sent_folder: String,
    pub allow_recipients: Vec<String>,
    pub auth: Auth,
    pub signature: Option<String>,
    pub origin: Option<String>,
}

fn preset(provider: &str) -> Option<(&'static str, u16, &'static str, u16, &'static str)> {
    // (imap_host, imap_port, smtp_host, smtp_port, sent_folder)
    Some(match provider {
        "qq" => ("imap.qq.com", 993, "smtp.qq.com", 465, "Sent Messages"),
        "exmail" => ("imap.exmail.qq.com", 993, "smtp.exmail.qq.com", 465, "Sent Messages"),
        "aliyun-enterprise" => ("imap.mxhichina.com", 993, "smtp.mxhichina.com", 465, "已发送"),
        "aliyun" => ("imap.aliyun.com", 993, "smtp.aliyun.com", 465, "已发送"),
        "gmail" => ("imap.gmail.com", 993, "smtp.gmail.com", 587, "[Gmail]/Sent Mail"),
        "outlook" => ("outlook.office365.com", 993, "smtp.office365.com", 587, "Sent Items"),
        _ => return None,
    })
}

pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MAILCTL_CONFIG") {
        return PathBuf::from(p);
    }
    let dir = home().join(".config/hermes-mail/accounts");
    if dir.is_dir() {
        dir
    } else {
        home().join(".config/hermes-mail/accounts.toml")
    }
}

pub fn default_audit_path() -> PathBuf {
    if let Ok(p) = std::env::var("MAILCTL_AUDIT") {
        return PathBuf::from(p);
    }
    home().join(".local/state/hermes-mail/audit.log")
}

pub fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

pub fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"))
}

/// 目录模式下的单文件结构：`[account]`（文件名即账号名）或 `[accounts.x]`
#[derive(Debug, Deserialize, Default)]
struct FileCfg {
    #[serde(default)]
    default: Defaults,
    account: Option<Account>,
    #[serde(default)]
    accounts: BTreeMap<String, Account>,
}

pub fn load(path: &Path) -> Result<Config> {
    if path.is_dir() {
        return load_dir(path);
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("读不到账号表 {}（模板：accounts.example.toml）", path.display()))?;
    let mut cfg: Config = toml::from_str(&text)
        .with_context(|| format!("账号表 {} 解析失败", path.display()))?;
    let origin = path.display().to_string();
    for acc in cfg.accounts.values_mut() {
        acc.origin = Some(origin.clone());
    }
    Ok(cfg)
}

/// 一个邮箱一个文件：`~/.config/hermes-mail/accounts/<名字>.toml`
pub fn load_dir(dir: &Path) -> Result<Config> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("读不到配置目录 {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("toml")).unwrap_or(false))
        .collect();
    files.sort();
    let mut cfg = Config::default();
    for f in &files {
        let text = std::fs::read_to_string(f)
            .with_context(|| format!("读不到 {}", f.display()))?;
        let file: FileCfg = toml::from_str(&text)
            .with_context(|| format!("解析 {} 失败（应为 [account] 或 [accounts.x]）", f.display()))?;
        if cfg.default.account.is_none() {
            cfg.default.account = file.default.account;
        }
        if cfg.default.confirm_before_send.is_none() {
            cfg.default.confirm_before_send = file.default.confirm_before_send;
        }
        if cfg.default.audit_log.is_none() {
            cfg.default.audit_log = file.default.audit_log;
        }
        let origin = f.display().to_string();
        if let Some(mut acc) = file.account {
            acc.origin = Some(origin.clone());
            let name = f.file_stem().unwrap_or_default().to_string_lossy().to_string();
            cfg.accounts.insert(name, acc);
        }
        for (name, mut acc) in file.accounts {
            acc.origin = Some(origin.clone());
            cfg.accounts.insert(name, acc);
        }
    }
    if cfg.accounts.is_empty() {
        bail!("目录 {} 里没有可用的账号（每个 *.toml 写一个 [account]）", dir.display());
    }
    Ok(cfg)
}

pub fn resolve(cfg: &Config, name: Option<&str>) -> Result<Resolved> {
    if cfg.accounts.is_empty() {
        bail!("账号表里没有任何 [accounts.*] 条目");
    }
    let picked = name
        .map(|s| s.to_string())
        .or_else(|| cfg.default.account.clone())
        .ok_or_else(|| anyhow!("未指定账号，账号表也没有 default.account。可选：{:?}", cfg.accounts.keys().collect::<Vec<_>>()))?;
    let acc = cfg
        .accounts
        .get(&picked)
        .ok_or_else(|| anyhow!("未知账号 {picked:?}。可选：{:?}", cfg.accounts.keys().collect::<Vec<_>>()))?;
    let provider = acc.provider.clone().unwrap_or_else(|| "custom".into());
    let p = preset(&provider);

    let imap_host = acc
        .imap_host
        .clone()
        .or_else(|| p.map(|x| x.0.to_string()))
        .ok_or_else(|| anyhow!("账号 {picked:?} 缺 imap_host（provider={provider:?} 无预设）"))?;
    let imap_port = acc.imap_port.or_else(|| p.map(|x| x.1)).unwrap_or(993);
    let smtp_host = acc
        .smtp_host
        .clone()
        .or_else(|| p.map(|x| x.2.to_string()))
        .ok_or_else(|| anyhow!("账号 {picked:?} 缺 smtp_host（provider={provider:?} 无预设）"))?;
    let smtp_port = acc.smtp_port.or_else(|| p.map(|x| x.3)).unwrap_or(465);
    let sent_folder = acc
        .sent_folder
        .clone()
        .or_else(|| p.map(|x| x.4.to_string()))
        .unwrap_or_else(|| "Sent".into());

    let imap_security = Security::parse(acc.imap_security.as_deref().unwrap_or("tls"), "imap")?;
    let smtp_security = Security::parse(acc.smtp_security.as_deref().unwrap_or("tls"), "smtp")?;

    // 明文只允许回环：防止把授权码裸奔在公网上
    if imap_security == Security::Plain && !is_loopback(&imap_host) {
        bail!("拒绝：账号 {picked:?} 的 imap_host={imap_host} 用明文，仅允许 127.0.0.1/localhost");
    }
    if smtp_security == Security::Plain && !is_loopback(&smtp_host) {
        bail!("拒绝：账号 {picked:?} 的 smtp_host={smtp_host} 用明文，仅允许 127.0.0.1/localhost");
    }

    let signature = resolve_signature(acc)?;

    Ok(Resolved {
        signature,
        origin: acc.origin.clone(),
        name: picked,
        address: acc.address.clone(),
        display_name: acc.display_name.clone(),
        provider,
        imap_host,
        imap_port,
        imap_security,
        smtp_host,
        smtp_port,
        smtp_security,
        sent_folder,
        allow_recipients: acc.allow_recipients.clone(),
        auth: acc.auth.clone().unwrap_or_default(),
    })
}

/// 签名：优先 inline，其次 signature_file（相对路径按账号文件所在目录解析）
fn resolve_signature(acc: &Account) -> Result<Option<String>> {
    if let Some(sig) = acc.signature.as_deref() {
        if !sig.trim().is_empty() {
            return Ok(Some(sig.to_string()));
        }
    }
    let Some(rel) = acc.signature_file.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let p = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        let base = acc
            .origin
            .as_deref()
            .map(|o| PathBuf::from(o))
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join(rel)
    };
    let text = std::fs::read_to_string(&p)
        .with_context(|| format!("签名文件读不到：{}", p.display()))?;
    Ok(Some(text))
}

impl Resolved {
    /// 白名单校验：空列表 = 不限制；支持 `domain.com` 与 `*.domain.com`。
    pub fn guard(&self, recipients: &[String]) -> Result<()> {
        let pats: Vec<String> = self
            .allow_recipients
            .iter()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        if pats.is_empty() {
            return Ok(());
        }
        if recipients.is_empty() {
            bail!("收件人解析为空，拒绝发送");
        }
        for addr in recipients {
            let a = addr.trim().to_ascii_lowercase();
            // 发给自己（自测/自留件）不经过白名单：不涉及第三方外发
            if a == self.address.to_ascii_lowercase() {
                continue;
            }
            let dom = a.rsplit('@').next().unwrap_or("");
            let ok = pats.iter().any(|pat| {
                if let Some(rest) = pat.strip_prefix("*.") {
                    dom == rest || dom.ends_with(&format!(".{rest}"))
                } else {
                    a == *pat || dom == pat || dom.ends_with(&format!(".{pat}"))
                }
            });
            if !ok {
                bail!(
                    "白名单拦截：「{}」不允许发往 {}。\n允许目标：{}\n要放开就改账号表的 allow_recipients，别用命令行绕。",
                    self.name,
                    addr,
                    self.allow_recipients.join(", ")
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(name: &str) -> Config {
        let toml = format!(
            r#"
[default]
account = "{name}"
[accounts.{name}]
provider = "exmail"
address = "a@b.com"
allow_recipients = ["b.com", "*.apple.com"]
"#
        );
        toml::from_str(&toml).unwrap()
    }

    #[test]
    fn preset_expands_endpoints() {
        let cfg = acc("w");
        let r = resolve(&cfg, None).unwrap();
        assert_eq!(r.imap_host, "imap.exmail.qq.com");
        assert_eq!(r.smtp_port, 465);
        assert_eq!(r.sent_folder, "Sent Messages");
    }

    #[test]
    fn guard_allows_matching_and_blocks_others() {
        let cfg = acc("w");
        let r = resolve(&cfg, None).unwrap();
        assert!(r.guard(&["x@apple.com".into()]).is_ok());
        assert!(r.guard(&["x@sub.apple.com".into()]).is_ok());
        assert!(r.guard(&["x@tencent.com".into()]).is_err());
    }

    #[test]
    fn loads_one_file_per_account_with_signatures() {
        let dir = std::env::temp_dir().join(format!("mailctl-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a-qq.toml"),
            r#"
[account]
provider = "qq"
address = "a@qq.com"
auth.keychain_service = "k1"
allow_recipients = ["apple.com"]
signature = "段文彬 / 运维负责人"
"#,
        )
        .unwrap();
        std::fs::write(dir.join("sig-work.txt"), "Work Sig\nTitle").unwrap();
        std::fs::write(
            dir.join("b-work.toml"),
            r#"
[account]
provider = "exmail"
address = "b@corp.com"
auth.keychain_service = "k2"
allow_recipients = ["corp.com"]
signature_file = "sig-work.txt"
"#,
        )
        .unwrap();

        let cfg = load(&dir).unwrap();
        assert_eq!(cfg.accounts.len(), 2);

        let a = resolve(&cfg, Some("a-qq")).unwrap();
        assert_eq!(a.origin.as_deref().unwrap(), dir.join("a-qq.toml").display().to_string());
        assert!(a.signature.as_deref().unwrap().contains("运维负责人"));
        assert!(a.guard(&["x@apple.com".into()]).is_ok());
        assert!(a.guard(&["x@corp.com".into()]).is_err(), "a 的白名单不应影响 b");

        let b = resolve(&cfg, Some("b-work")).unwrap();
        assert_eq!(b.signature.as_deref().unwrap().trim(), "Work Sig\nTitle");
        assert!(b.guard(&["x@corp.com".into()]).is_ok());
        assert!(b.guard(&["x@apple.com".into()]).is_err(), "b 的白名单不应影响 a");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn guard_allows_self_send() {
        let cfg = acc("w");
        let r = resolve(&cfg, None).unwrap();
        assert!(r.guard(&["a@b.com".into()]).is_ok(), "自己发给自己应放行");
        assert!(r.guard(&["x@tencent.com".into()]).is_err());
    }

    #[test]
    fn plaintext_outside_loopback_is_refused() {
        let toml = r#"
[accounts.x]
provider = "custom"
address = "a@b.com"
imap_host = "imap.example.com"
imap_security = "plain"
smtp_host = "smtp.example.com"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        let err = resolve(&cfg, Some("x")).unwrap_err().to_string();
        assert!(err.contains("明文"), "err: {err}");
    }

    #[test]
    fn plaintext_on_loopback_is_allowed() {
        let toml = r#"
[accounts.x]
provider = "custom"
address = "a@b.com"
imap_host = "127.0.0.1"
imap_security = "plain"
smtp_host = "127.0.0.1"
smtp_security = "plain"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(resolve(&cfg, Some("x")).is_ok());
    }

    #[test]
    fn custom_requires_hosts() {
        let toml = r#"
[accounts.x]
provider = "custom"
address = "a@b.com"
"#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert!(resolve(&cfg, Some("x")).is_err());
    }
}
