use redb::{Database, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::error::Result;

/// 用户信息表定义
const USER_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("users");

/// 用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    /// 用户 ID（唯一标识）
    pub id: String,
    /// 用户昵称
    pub nickname: String,
    /// 头像 URL 或路径
    pub avatar: Option<String>,
    /// 创建时间
    pub created_at: i64,
    /// 最后登录时间
    pub last_login: i64,
}

impl UserInfo {
    /// 创建新用户
    pub fn new(nickname: String) -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            nickname,
            avatar: None,
            created_at: now,
            last_login: now,
        }
    }

    /// 更新最后登录时间
    pub fn update_last_login(&mut self) {
        self.last_login = chrono::Utc::now().timestamp();
    }
}

/// 用户数据库
pub struct UserDatabase {
    db: Database,
}

impl UserDatabase {
    /// 打开或创建用户数据库
    pub fn open(path: PathBuf) -> Result<Self> {
        // 确保目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| crate::error::AppError::Io(e))?;
        }

        let db = Database::create(path)
            .map_err(|e| crate::error::AppError::Other(format!("Failed to create database: {}", e)))?;

        Ok(Self { db })
    }

    /// 保存用户信息
    pub fn save_user(&self, user: &UserInfo) -> Result<()> {
        let write_txn = self.db.begin_write()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to begin write: {}", e)))?;

        {
            let mut table = write_txn.open_table(USER_TABLE)
                .map_err(|e| crate::error::AppError::Other(format!("Failed to open table: {}", e)))?;

            let user_bytes = serde_json::to_vec(user)
                .map_err(|e| crate::error::AppError::Other(format!("Failed to serialize user: {}", e)))?;

            table.insert("current_user", user_bytes.as_slice())
                .map_err(|e| crate::error::AppError::Other(format!("Failed to insert user: {}", e)))?;
        }

        write_txn.commit()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to commit: {}", e)))?;

        log::info!("User saved: {} ({})", user.nickname, user.id);
        Ok(())
    }

    /// 获取当前用户信息
    pub fn get_current_user(&self) -> Result<Option<UserInfo>> {
        let read_txn = self.db.begin_read()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to begin read: {}", e)))?;

        let table = read_txn.open_table(USER_TABLE)
            .map_err(|e| crate::error::AppError::Other(format!("Failed to open table: {}", e)))?;

        let user_bytes = table.get("current_user")
            .map_err(|e| crate::error::AppError::Other(format!("Failed to get user: {}", e)))?;

        if let Some(bytes) = user_bytes {
            let user: UserInfo = serde_json::from_slice(bytes.value())
                .map_err(|e| crate::error::AppError::Other(format!("Failed to deserialize user: {}", e)))?;
            Ok(Some(user))
        } else {
            Ok(None)
        }
    }

    /// 检查是否有用户信息
    pub fn has_user(&self) -> bool {
        self.get_current_user().ok().flatten().is_some()
    }

    /// 删除当前用户信息
    pub fn delete_user(&self) -> Result<()> {
        let write_txn = self.db.begin_write()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to begin write: {}", e)))?;

        {
            let mut table = write_txn.open_table(USER_TABLE)
                .map_err(|e| crate::error::AppError::Other(format!("Failed to open table: {}", e)))?;

            table.remove("current_user")
                .map_err(|e| crate::error::AppError::Other(format!("Failed to remove user: {}", e)))?;
        }

        write_txn.commit()
            .map_err(|e| crate::error::AppError::Other(format!("Failed to commit: {}", e)))?;

        log::info!("User deleted");
        Ok(())
    }
}
