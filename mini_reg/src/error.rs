#[derive(Debug, thiserror::Error)]
pub enum RegError {
    #[error("I/O / 文件操作: {0}")]
    Io(#[from] std::io::Error),
    #[error("SQLite / 本地状态库: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("JSON / 数据格式: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Invalid batch TOML / 批次格式不符，请检查字段与类型；原文未写入日志")]
    Toml(#[from] toml::de::Error),
    #[error("HTTP / 本地浏览器连接: {0}")]
    Http(#[from] reqwest::Error),
    #[error("WebSocket / 浏览器协议连接失败")]
    WebSocket(#[from] Box<tungstenite::Error>),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, RegError>;

impl From<tungstenite::Error> for RegError {
    fn from(error: tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(error))
    }
}

pub fn invalid(message: impl Into<String>) -> RegError {
    RegError::Invalid(message.into())
}
