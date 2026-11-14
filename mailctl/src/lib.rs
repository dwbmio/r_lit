//! mailctl —— 多邮箱 IMAP/SMTP 分发器（纯 Rust，单二进制，无解释器依赖）。

pub mod audit;
pub mod config;
pub mod cred;
pub mod imap;
pub mod mime;
pub mod net;
pub mod smtp;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 能力清单：直接内嵌仓库根的 capabilities.json，保证二进制与文档同源
pub const CAPABILITIES: &str = include_str!("../capabilities.json");
