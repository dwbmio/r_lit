//! 凭证获取：命令行 > 钥匙串 > 环境变量 > 交互输入。永不写盘、不打印。

use crate::config::Resolved;
use anyhow::{bail, Result};
use std::process::Command;

pub fn keychain_lookup(service: &str) -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", service, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

pub fn password(acc: &Resolved, cli: Option<String>, allow_prompt: bool) -> Result<String> {
    if let Some(v) = cli.filter(|s| !s.is_empty()) {
        return Ok(v);
    }
    if let Some(svc) = acc.auth.keychain_service.as_deref().filter(|s| !s.is_empty()) {
        if let Some(v) = keychain_lookup(svc) {
            return Ok(v);
        }
        eprintln!(
            "提示：钥匙串里没有 service={svc} 的条目。录入：\n  security add-generic-password -a {} -s {svc} -w '<授权码>'",
            acc.address
        );
    }
    if let Some(env_name) = acc.auth.env.as_deref().filter(|s| !s.is_empty()) {
        if let Ok(v) = std::env::var(env_name) {
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }
    if allow_prompt {
        if let Some(v) = prompt_hidden(&format!("「{}」{} 的授权码/应用密码：", acc.name, acc.address)) {
            if !v.is_empty() {
                return Ok(v);
            }
        }
    }
    bail!(
        "缺少凭证：请设置钥匙串 keychain_service、环境变量 auth.env，或加 --prompt-password"
    )
}

/// 借 bash 的 read -s 关闭回显，避免明文出现在终端里。
fn prompt_hidden(prompt: &str) -> Option<String> {
    let script = format!(
        "printf '%s' {} >/dev/tty; IFS= read -rs v </dev/tty; printf '%s' \"$v\"",
        shell_quote(prompt)
    );
    let out = Command::new("bash").args(["-c", &script]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if v.is_empty() {
        None
    } else {
        Some(v)
    }
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
