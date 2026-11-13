//! Native HTTP/WebSocket CDP transport; no browser downloads or script runtime.
use crate::{
    error::{invalid, Result},
    model::{App, Platform},
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::Read,
    net::{IpAddr, SocketAddr, TcpStream},
    path::{Component, Path, PathBuf},
    time::Duration,
};
use tungstenite::{Message, WebSocket};
use url::Url;

const COMPANY_DOMAIN: &str = "gamesci-lite.com";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub user_data_dir: PathBuf,
    pub profile_directory: String,
    pub port: u16,
    pub browser_path: String,
}

#[derive(Deserialize)]
struct Version {
    #[serde(rename = "Browser")]
    browser: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket: String,
}

#[derive(Deserialize)]
struct Target {
    id: String,
    url: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Tab {
    pub id: String,
    pub origin: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selectors {
    pub name: Option<String>,
    pub summary: Option<String>,
}

impl Binding {
    pub fn from_profile(directory: &Path, profile: &str) -> Result<Self> {
        let user_data_dir = directory.canonicalize()?;
        let mut components = Path::new(profile).components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(invalid(
                "Specify one profile directory name / 请明确指定企业浏览器的配置目录名",
            ));
        }
        verify_company_profile(&user_data_dir, profile)?;
        let (port, browser_path) = active_port(&user_data_dir)?;
        Ok(Self {
            user_data_dir,
            profile_directory: profile.into(),
            port,
            browser_path,
        })
    }

    pub fn verify(&self) -> Result<()> {
        verify_company_profile(&self.user_data_dir, &self.profile_directory)?;
        let (port, path) = active_port(&self.user_data_dir)?;
        if port != self.port || path != self.browser_path {
            return Err(invalid(
                "Browser restarted; bind again / 浏览器已重启，请重新绑定，未连接其他进程",
            ));
        }
        Ok(())
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let mut file = std::fs::File::open(path)?;
    if file.metadata()?.len() > 16 * 1024 * 1024 {
        return Err(invalid("Profile metadata is too large / 浏览器元信息过大"));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(Into::into)
}

fn enterprise_email(value: &str) -> bool {
    value
        .rsplit_once('@')
        .is_some_and(|(_, domain)| domain.eq_ignore_ascii_case(COMPANY_DOMAIN))
}

/// Inspect account metadata only. Cookies, passwords and token stores are never read.
pub fn verify_company_profile(directory: &Path, profile: &str) -> Result<()> {
    let prefs = read_json(&directory.join(profile).join("Preferences"))?;
    let accounts = prefs
        .get("account_info")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Enterprise account cannot be verified / 无法核实企业浏览器身份"))?;
    if accounts.is_empty()
        || accounts.iter().any(|a| {
            !a.get("email")
                .and_then(Value::as_str)
                .is_some_and(enterprise_email)
                || a.get("hosted_domain").and_then(Value::as_str) != Some(COMPANY_DOMAIN)
        })
    {
        return Err(invalid("Only gamesci-lite.com enterprise Chrome is allowed / 只允许企业 Chrome，不连接个人浏览器"));
    }
    let state = read_json(&directory.join("Local State"))?;
    if state.pointer("/profile/last_used").and_then(Value::as_str) != Some(profile) {
        return Err(invalid(
            "Active profile does not match / 当前浏览器配置与指定配置不符",
        ));
    }
    let cache = state
        .pointer("/profile/info_cache")
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("Profile inventory missing / 无法核实浏览器配置清单"))?;
    // Reject shared user-data directories with another profile; never expose a personal tab.
    if cache.len() != 1 || !cache.contains_key(profile) {
        return Err(invalid("Use a dedicated enterprise user-data directory / 请使用企业浏览器独立数据目录，禁止混用个人配置"));
    }
    Ok(())
}

fn active_port(directory: &Path) -> Result<(u16, String)> {
    let content = std::fs::read_to_string(directory.join("DevToolsActivePort"))?;
    let mut lines = content.lines();
    let port = lines
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|p| *p != 0)
        .ok_or_else(|| invalid("Invalid DevToolsActivePort / 企业 Chrome 未提供有效调试端口"))?;
    let path = lines
        .next()
        .filter(|p| {
            p.starts_with("/devtools/browser/")
                && p.len() > 18
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
        })
        .ok_or_else(|| invalid("Invalid browser fingerprint / 浏览器进程标识不符"))?;
    Ok((port, path.into()))
}

pub fn checked_portal(value: &str, platform: Platform) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| invalid("Invalid page URL / 页面地址不符"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port_or_known_default() != Some(443)
        || !url
            .host_str()
            .is_some_and(|h| platform.hosts().contains(&h))
    {
        return Err(invalid(
            "Page is outside the official platform / 当前页不是该平台官方后台，未操作",
        ));
    }
    Ok(url)
}

pub fn checked_websocket(value: &str, port: u16) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| invalid("Invalid CDP URL / 浏览器协议地址不符"))?;
    let loopback = url
        .host_str()
        .and_then(|h| h.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_loopback());
    if url.scheme() != "ws"
        || !loopback
        || url.port() != Some(port)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/devtools/")
    {
        return Err(invalid(
            "CDP must use the bound loopback port / 仅允许已绑定的本机浏览器端口",
        ));
    }
    Ok(url)
}

pub struct Browser {
    binding: Binding,
    client: Client,
}

impl Browser {
    pub fn connect(binding: Binding) -> Result<Self> {
        binding.verify()?;
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let browser = Self { binding, client };
        let version: Version = serde_json::from_value(browser.get("/json/version")?)?;
        let url = checked_websocket(&version.websocket, browser.binding.port)?;
        if !version.browser.contains("Chrome/") || url.path() != browser.binding.browser_path {
            return Err(invalid(
                "Chrome process fingerprint mismatch / 企业 Chrome 进程指纹不符",
            ));
        }
        Ok(browser)
    }

    fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("http://127.0.0.1:{}{path}", self.binding.port))
            .send()?
            .error_for_status()?;
        let mut bytes = Vec::new();
        response.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err(invalid("CDP reply too large / 浏览器返回过大"));
        }
        serde_json::from_slice(&bytes).map_err(Into::into)
    }

    fn targets(&self) -> Result<Vec<Target>> {
        self.binding.verify()?;
        serde_json::from_value(self.get("/json/list")?).map_err(Into::into)
    }

    pub fn tabs(&self) -> Result<Vec<Tab>> {
        let mut tabs = Vec::new();
        for target in self.targets()? {
            if target.kind != "page" {
                continue;
            }
            for platform in [
                Platform::Wechat,
                Platform::Douyin,
                Platform::Qq,
                Platform::Alipay,
            ] {
                if let Ok(url) = checked_portal(&target.url, platform) {
                    tabs.push(Tab {
                        id: target.id.clone(),
                        origin: url.origin().ascii_serialization(),
                    });
                    break;
                }
            }
        }
        Ok(tabs)
    }

    pub fn open(&self, platform: Platform) -> Result<String> {
        self.binding.verify()?;
        let endpoint = format!(
            "ws://127.0.0.1:{}{}",
            self.binding.port, self.binding.browser_path
        );
        let mut cdp = Cdp::connect(&endpoint, self.binding.port)?;
        let response = cdp.call("Target.createTarget", json!({"url": platform.portal()}))?;
        response
            .get("targetId")
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| invalid("No tab id returned / 未获得新页面编号"))
    }

    pub fn form(
        &self,
        app: &App,
        tab_id: &str,
        selectors: &Selectors,
        apply: bool,
    ) -> Result<Value> {
        let targets = self.targets()?;
        let tab = targets
            .into_iter()
            .find(|t| t.id == tab_id && t.kind == "page")
            .ok_or_else(|| {
                invalid("Tab no longer exists / 页面已关闭，请重新打开或明确指定页面")
            })?;
        checked_portal(&tab.url, app.platform)?;
        let endpoint = tab
            .websocket
            .ok_or_else(|| invalid("Tab is not inspectable / 页面未提供调试连接"))?;
        let mut cdp = Cdp::connect(&endpoint, self.binding.port)?;
        let config = json!({
            "hosts": app.platform.hosts(), "name": app.name, "summary": app.summary,
            "selectors": selectors, "apply": apply,
        });
        let expression = format!("({})({})", include_str!("form.js"), config);
        let result = cdp.call(
            "Runtime.evaluate",
            json!({
                "expression": expression, "returnByValue": true, "awaitPromise": true,
            }),
        )?;
        if result.get("exceptionDetails").is_some() {
            return Err(invalid(
                "Form script failed; review the page / 填表失败，请检查页面，未重试",
            ));
        }
        let value = result
            .pointer("/result/value")
            .ok_or_else(|| invalid("Form result missing / 未获得填表结果"))?;
        if value.get("ok").and_then(Value::as_bool) != Some(true) {
            let code = value
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let safe = match code {
                "origin" => "页面已离开官方后台",
                "missing_name" => "未识别名称输入框，请先进入创建页或设置明确选择器",
                "ambiguous_name" | "ambiguous_summary" => "输入框不唯一，请设置明确选择器",
                "occupied_name" | "occupied_summary" => "已有不同内容，未覆盖，请人工检查",
                "invalid_selector" => "选择器不符或不是可写文本框",
                "maxlength" => "内容超出当前输入框长度限制",
                "write_failed" => "页面未接受输入，请人工检查",
                _ => "页面结构不符，请人工检查",
            };
            return Err(invalid(format!("Form requires review / {safe}")));
        }
        Ok(value.clone())
    }
}

struct Cdp {
    socket: WebSocket<TcpStream>,
    next_id: u64,
}

impl Cdp {
    fn connect(endpoint: &str, port: u16) -> Result<Self> {
        let url = checked_websocket(endpoint, port)?;
        let host: IpAddr = url
            .host_str()
            .and_then(|h| h.parse().ok())
            .ok_or_else(|| invalid("CDP loopback address missing / 缺少浏览器本机地址"))?;
        let stream =
            TcpStream::connect_timeout(&SocketAddr::new(host, port), Duration::from_secs(8))?;
        stream.set_read_timeout(Some(Duration::from_secs(8)))?;
        stream.set_write_timeout(Some(Duration::from_secs(8)))?;
        let (socket, _) = tungstenite::client(url.as_str(), stream)
            .map_err(|_| invalid("CDP handshake failed / 浏览器协议握手失败"))?;
        Ok(Self { socket, next_id: 0 })
    }

    fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        log::debug!("CDP {method}");
        self.socket.send(Message::Text(serde_json::to_string(
            &json!({"id": id, "method": method, "params": params}),
        )?))?;
        for _ in 0..512 {
            let message = self.socket.read()?;
            if message.len() > 1024 * 1024 {
                return Err(invalid("CDP message too large / 浏览器消息过大"));
            }
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text)?;
                if value.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if value.get("error").is_some() {
                    return Err(invalid(format!(
                        "CDP operation failed / 浏览器操作失败: {method}"
                    )));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
        }
        Err(invalid("CDP response limit reached / 浏览器响应超限"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cdp_handshake_and_response_correlation(
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let server = std::thread::spawn(move || -> std::result::Result<(), String> {
            let (stream, _) = listener.accept().map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            let mut socket = tungstenite::accept(stream).map_err(|e| e.to_string())?;
            let request = socket.read().map_err(|e| e.to_string())?;
            let request: Value =
                serde_json::from_str(request.to_text().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let id = request
                .get("id")
                .and_then(Value::as_u64)
                .ok_or("missing request id")?;
            socket
                .send(Message::Text(
                    json!({"method":"Runtime.event","params":{}}).to_string(),
                ))
                .map_err(|e| e.to_string())?;
            socket
                .send(Message::Text(
                    json!({"id":id+1,"result":{"accepted":false}}).to_string(),
                ))
                .map_err(|e| e.to_string())?;
            socket
                .send(Message::Text(
                    json!({"id":id,"result":{"accepted":true}}).to_string(),
                ))
                .map_err(|e| e.to_string())?;
            Ok(())
        });
        let mut client = Cdp::connect(&format!("ws://127.0.0.1:{port}/devtools/page/test"), port)?;
        let reply = client.call("Runtime.evaluate", json!({"expression":"fixture"}))?;
        assert_eq!(reply["accepted"], true);
        server
            .join()
            .map_err(|_| std::io::Error::other("mock CDP thread panicked"))??;
        Ok(())
    }
}
