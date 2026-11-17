# 错误处理指南

## Murmur 的错误类型

Murmur 使用 `Result<T>` 返回类型，所有操作都可能返回错误。

```rust
pub type Result<T> = std::result::Result<T, Error>;
```

## 核心 API 的错误处理

### KV 操作

```rust
// put() 返回 Result<()>
pub async fn put(&self, key: &str, value: &[u8]) -> Result<()>

// get() 返回 Result<Option<Vec<u8>>>
pub async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>

// delete() 返回 Result<()>
pub async fn delete(&self, key: &str) -> Result<()>
```

### 使用示例

```rust
// 处理 put 错误
match swarm.put("key", b"value").await {
    Ok(()) => println!("✅ 成功"),
    Err(e) => println!("❌ 错误: {}", e),
}

// 处理 get 错误
match swarm.get("key").await {
    Ok(Some(value)) => println!("找到: {:?}", value),
    Ok(None) => println!("未找到"),
    Err(e) => println!("❌ 错误: {}", e),
}
```

## 文件操作的错误类型 (file-ops feature)

### 新增错误类型

```rust
pub enum Error {
    // ... 其他错误类型

    /// 文件太大
    #[error("File too large: {size} bytes (max: {max} bytes)")]
    FileTooLarge { size: usize, max: usize },

    /// 版本冲突
    #[error("Version conflict: expected {expected}, current {current}")]
    VersionConflict { expected: u64, current: u64 },
}
```

### 文件大小限制

```rust
/// 最大文件大小：10 MB
pub const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;
```

### 错误处理示例

#### 1. 处理大文件错误

```rust
use murmur::{Error, FileOps};

match swarm.put_file(Path::new("large_file.bin")).await {
    Ok(key) => println!("✅ 上传成功: {}", key),
    Err(Error::FileTooLarge { size, max }) => {
        println!("❌ 文件太大: {} bytes (最大: {} bytes)", size, max);
        println!("   提示: 请使用分块上传或压缩文件");
    }
    Err(e) => println!("❌ 其他错误: {}", e),
}
```

#### 2. 处理版本冲突

```rust
use murmur::{Error, FileOps};

let meta = swarm.file_metadata(&key).await?.unwrap();

match swarm.put_file_with_version(
    Path::new("doc.txt"),
    Some(meta.version)
).await {
    Ok(_) => println!("✅ 上传成功"),
    Err(Error::VersionConflict { expected, current }) => {
        println!("❌ 版本冲突:");
        println!("   期望版本: {}", expected);
        println!("   当前版本: {}", current);
        println!("   提示: 请先下载最新版本并合并");

        // 获取最新版本
        let latest_meta = swarm.file_metadata(&key).await?.unwrap();
        println!("   最新版本作者: {}", latest_meta.author);
        println!("   最新版本时间: {}", latest_meta.modified);
    }
    Err(e) => println!("❌ 其他错误: {}", e),
}
```

#### 3. 完整的错误处理流程

```rust
use murmur::{Error, FileOps};
use std::path::Path;

async fn upload_with_retry(
    swarm: &Swarm,
    file_path: &Path,
    max_retries: u32
) -> Result<String> {
    let mut retries = 0;

    loop {
        // 获取当前版本
        let key = format!("file:data:{}", file_path.file_name().unwrap().to_string_lossy());
        let current_version = if let Ok(Some(meta)) = swarm.file_metadata(&key).await {
            Some(meta.version)
        } else {
            None
        };

        // 尝试上传
        match swarm.put_file_with_version(file_path, current_version).await {
            Ok(key) => return Ok(key),

            Err(Error::FileTooLarge { size, max }) => {
                // 文件太大，无法重试
                return Err(Error::FileTooLarge { size, max });
            }

            Err(Error::VersionConflict { expected, current }) => {
                retries += 1;
                if retries >= max_retries {
                    return Err(Error::VersionConflict { expected, current });
                }

                println!("⚠️ 版本冲突，重试 {}/{}", retries, max_retries);
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                // 循环会重新获取版本并重试
            }

            Err(e) => return Err(e),
        }
    }
}
```

## 所有错误类型

```rust
pub enum Error {
    /// 存储错误 (SQLite)
    Storage(rusqlite::Error),

    /// 网络错误
    Network(String),

    /// 序列化错误
    Serialization(String),

    /// 选举错误
    Election(String),

    /// 同步错误
    Sync(String),

    /// IO 错误
    Io(std::io::Error),

    /// 未找到
    NotFound(String),

    /// 文件太大 (file-ops feature)
    FileTooLarge { size: usize, max: usize },

    /// 版本冲突 (file-ops feature)
    VersionConflict { expected: u64, current: u64 },

    /// 其他错误
    Other(String),
}
```

## 错误处理最佳实践

### 1. 使用 ? 操作符传播错误

```rust
async fn my_function(swarm: &Swarm) -> Result<()> {
    swarm.put("key", b"value").await?;  // 自动传播错误
    Ok(())
}
```

### 2. 使用 match 处理特定错误

```rust
match swarm.put_file(path).await {
    Ok(key) => { /* 成功 */ }
    Err(Error::FileTooLarge { .. }) => { /* 处理大文件 */ }
    Err(Error::VersionConflict { .. }) => { /* 处理冲突 */ }
    Err(e) => { /* 处理其他错误 */ }
}
```

### 3. 使用 map_err 转换错误

```rust
swarm.put("key", b"value").await
    .map_err(|e| format!("上传失败: {}", e))?;
```

### 4. 提供用户友好的错误信息

```rust
match swarm.put_file(path).await {
    Err(Error::FileTooLarge { size, max }) => {
        eprintln!("文件太大: {:.2} MB (最大: {:.2} MB)",
            size as f64 / 1024.0 / 1024.0,
            max as f64 / 1024.0 / 1024.0
        );
        eprintln!("建议:");
        eprintln!("  1. 压缩文件");
        eprintln!("  2. 使用分块上传");
        eprintln!("  3. 使用外部存储（如 S3）");
    }
    Err(e) => eprintln!("错误: {}", e),
    Ok(_) => {}
}
```

## 上游处理示例

### 在应用层捕获错误

```rust
// 在 group_vibe_workbench 中
use murmur::{Error, FileOps};

impl SharedFile {
    pub async fn update_content(&self, content: String) -> Result<()> {
        match self.swarm.put_file_with_version(
            &self.local_path,
            Some(self.current_version)
        ).await {
            Ok(_) => {
                self.current_version += 1;
                Ok(())
            }
            Err(Error::FileTooLarge { size, max }) => {
                // 显示 UI 提示
                show_error_dialog(&format!(
                    "文件太大: {} bytes\n最大: {} bytes",
                    size, max
                ));
                Err(Error::FileTooLarge { size, max })
            }
            Err(Error::VersionConflict { expected, current }) => {
                // 显示冲突解决 UI
                show_conflict_dialog(expected, current);
                Err(Error::VersionConflict { expected, current })
            }
            Err(e) => Err(e),
        }
    }
}
```

## 总结

✅ **所有 API 都返回 Result**
- `put()` → `Result<()>`
- `get()` → `Result<Option<Vec<u8>>>`
- `put_file()` → `Result<String>`
- `put_file_with_version()` → `Result<String>`

✅ **明确的错误类型**
- `FileTooLarge` - 文件超过 10MB
- `VersionConflict` - 版本不匹配
- `Io` - 文件读写错误
- `Network` - 网络错误
- 等等...

✅ **上游可以精确处理**
- 使用 `match` 匹配特定错误
- 提供用户友好的错误信息
- 实现重试逻辑
- 显示 UI 提示

✅ **大文件自动拒绝**
- 超过 10MB 立即返回 `FileTooLarge`
- 不会尝试上传
- 上游可以提示用户使用其他方案
