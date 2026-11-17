use crate::error::{invalid, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Wechat,
    Douyin,
    Qq,
    Alipay,
}

impl Platform {
    pub fn key(self) -> &'static str {
        match self {
            Self::Wechat => "wechat",
            Self::Douyin => "douyin",
            Self::Qq => "qq",
            Self::Alipay => "alipay",
        }
    }

    pub fn portal(self) -> &'static str {
        match self {
            Self::Wechat => "https://mp.weixin.qq.com/",
            Self::Douyin => "https://developer.open-douyin.com/",
            Self::Qq => "https://q.qq.com/",
            Self::Alipay => "https://open.alipay.com/",
        }
    }

    pub fn hosts(self) -> &'static [&'static str] {
        match self {
            Self::Wechat => &["mp.weixin.qq.com"],
            Self::Douyin => &["developer.open-douyin.com", "open.douyin.com"],
            Self::Qq => &["q.qq.com"],
            Self::Alipay => &[
                "open.alipay.com",
                "openhome.alipay.com",
                "mini.open.alipay.com",
            ],
        }
    }

    pub fn validate_app_id(self, value: &str) -> Result<()> {
        let valid = match self {
            Self::Wechat => {
                value.len() == 18
                    && value.starts_with("wx")
                    && value[2..].bytes().all(|b| b.is_ascii_hexdigit())
            }
            Self::Douyin => {
                (10..=64).contains(&value.len())
                    && value.starts_with("tt")
                    && value.bytes().all(|b| b.is_ascii_alphanumeric())
            }
            Self::Qq => {
                (5..=32).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_digit())
            }
            Self::Alipay => value.len() == 16 && value.bytes().all(|b| b.is_ascii_digit()),
        };
        if !valid {
            return Err(invalid("Invalid platform AppID / 应用编号格式不符"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    MiniProgram,
    MiniGame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub id: String,
    pub platform: Platform,
    pub kind: AppKind,
    pub name: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema_version: u32,
    pub id: String,
    pub company: String,
    pub apps: Vec<App>,
}

pub fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 80
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

impl Batch {
    pub fn read(path: &Path) -> Result<Self> {
        let batch: Self = toml::from_str(&std::fs::read_to_string(path)?)?;
        batch.validate()?;
        Ok(batch)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || !valid_key(&self.id) {
            return Err(invalid("Invalid batch version/id / 批次版本或编号不符"));
        }
        if self.company.trim().is_empty() || self.company.len() > 512 {
            return Err(invalid("Company is required / 请填写自家公司名称"));
        }
        if self.apps.is_empty() || self.apps.len() > 1000 {
            return Err(invalid(
                "A batch needs 1..1000 apps / 每批需有 1..1000 个应用",
            ));
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        for app in &self.apps {
            if !valid_key(&app.id) || !ids.insert(&app.id) {
                return Err(invalid("Invalid/duplicate task id / 任务编号不符或重复"));
            }
            if app.name.trim().is_empty()
                || app.name != app.name.trim()
                || app.name.chars().count() > 128
                || app.summary.chars().count() > 2000
            {
                return Err(invalid("Invalid name/summary / 名称或简介不符"));
            }
            if !names.insert((app.platform.key(), &app.name)) {
                return Err(invalid("Duplicate platform/name / 同平台应用名称重复"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub batch: String,
    pub company: String,
    pub app: App,
    pub phase: String,
    pub tab_id: Option<String>,
    pub app_id: Option<String>,
    pub evidence: Option<String>,
    pub platform_verified: bool,
}

pub const EXAMPLE: &str = r#"schema_version = 1
id = "company-apps-001"
company = "Your company"

[[apps]]
id = "wechat-demo"
platform = "wechat"
kind = "mini_program"
name = "Replace with your app name"
summary = "Replace with the app description"

[[apps]]
id = "douyin-demo"
platform = "douyin"
kind = "mini_program"
name = "Replace with your app name"
summary = "Replace with the app description"

[[apps]]
id = "qq-demo"
platform = "qq"
kind = "mini_program"
name = "Replace with your app name"
summary = "Replace with the app description"

[[apps]]
id = "alipay-demo"
platform = "alipay"
kind = "mini_program"
name = "Replace with your app name"
summary = "Replace with the app description"
"#;
