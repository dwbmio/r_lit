# Murmur

> 分布式 P2P 协作库。零配置，零服务器。

## 简介

一个 Rust 库，让设备之间无需中心服务器即可同步数据。同一 WiFi 下的设备自动发现、自动同步。

基于 CRDT 实现无冲突合并，内置版本控制，支持多种存储后端。

## 为什么用这个？

- **无需服务器** — 纯 P2P，局域网直连
- **零配置** — mDNS 自动发现
- **CRDT 同步** — Automerge 无冲突复制
- **冲突检测与解决** — 自动锁定 + 事件驱动的解决流程
- **内置版本控制** — 数据时间旅行（可选）
- **审计追踪** — 文件粒度的完整变更记录（作者、时间戳、操作类型）
- **Rust 原生** — 高性能、内存安全

适合构建协作应用、离线优先工具、或任何需要无云同步的场景。

## 快速开始

添加依赖：

```toml
[dependencies]
murmur = { path = "../murmur" }
tokio = { version = "1", features = ["full"] }
```

基本使用：

```rust
use murmur::Swarm;

#[tokio::main]
async fn main() -> Result<()> {
    let swarm = Swarm::builder()
        .storage_path("./data")
        .build()
        .await?;

    swarm.start().await?;

    swarm.put("user:alice", b"Alice").await?;

    if let Some(value) = swarm.get("user:alice").await? {
        println!("Found: {}", String::from_utf8_lossy(&value));
    }

    Ok(())
}
```

数据自动同步到网络中的所有节点。

## 核心功能

### P2P 网络
基于 `iroh-net`，自动 NAT 穿透，mDNS 局域网发现。

### CRDT 同步
使用 Automerge，多节点并发修改无冲突。

### Leader 选举
Bully 算法自动协调者选举，用于协调分布式操作。

### 可插拔存储
redb（默认）、SQLite、RocksDB 三选一，均提供持久化本地存储。

## 文件操作（可选）

启用 `file-ops` feature 获得带版本控制的文件同步：

```toml
murmur = { path = "../murmur", features = ["file-ops"] }
```

```rust
use murmur::{Swarm, FileOps};
use std::path::Path;

// 上传文件（自动版本化）
let key = swarm.put_file(Path::new("document.txt")).await?;

// 下载最新版
swarm.get_file(&key, Path::new("output.txt")).await?;

// 时间旅行 — 获取第 3 版
swarm.get_file_version(&key, 3, Path::new("old.txt")).await?;

// 查看历史
let history = swarm.file_history(&key).await?;
for entry in history {
    println!("v{} by {} at {}", entry.version, entry.author, entry.timestamp);
}
```

**具备能力：**
- 自动版本化（每次写入创建新版本）
- 完整审计追踪（谁改了什么、什么时候改的），通过 `audit_trail()` 查询
- 分布式文件锁的冲突检测
- 事件驱动解决：通过 `subscribe()` 接收 `ConflictDetected` / `ConflictResolved`
- 文件大小限制（默认 10MB）

### 冲突解决流程

当两个节点同时写同一文件时：

1. CRDT 检测到并发写入 → 文件在所有节点 **锁定**
2. 所有节点收到 `SwarmEvent::ConflictDetected`
3. 指定的解决者调用 `resolve_conflict()`（KeepLocal / KeepRemote / MergeWith）
4. 所有节点收到 `SwarmEvent::ConflictResolved`，文件解锁，同步恢复

```rust
use murmur::{Swarm, SwarmEvent, ConflictResolution, FileOps};

let mut events = swarm.subscribe();
loop {
    match events.recv().await? {
        SwarmEvent::ConflictDetected { file_name, resolver_node, .. } => {
            if resolver_node == swarm.node_id().await {
                swarm.resolve_conflict(&file_name, ConflictResolution::KeepLocal).await?;
            }
        }
        SwarmEvent::ConflictResolved { file_name, new_version, .. } => {
            println!("{} 已解决 → v{}", file_name, new_version);
        }
        _ => {}
    }
}
```

## 架构

```
┌─────────────────────────────────────┐
│  你的应用                            │
├─────────────────────────────────────┤
│  Murmur API (KV + 文件操作)         │
├─────────────────────────────────────┤
│  CRDT 层 (Automerge)               │
├─────────────────────────────────────┤
│  P2P 网络 (iroh-net)               │
└─────────────────────────────────────┘
```

分层设计：应用层调用 API，API 用 CRDT 同步，CRDT 通过 P2P 传输。

## 使用场景

- **协作编辑** — 多人实时编辑文档
- **局域网游戏** — 同网络内同步游戏状态
- **离线优先应用** — 本地工作，连接后同步
- **配置管理** — 多机器间同步配置
- **团队数据共享** — 无需云服务的团队内数据共享

## 示例

```bash
# 基础键值存储
cargo run --example basic

# 群聊应用
cargo run --example group_chat

# 带版本控制的文件同步
cargo run --example file_sync --features file-ops

# 性能基准测试
cargo run --example benchmark --release
```

## 配置

### 存储后端

```toml
# redb（默认，快速嵌入式）
murmur = { path = "../murmur" }

# SQLite（成熟稳定）
murmur = { path = "../murmur", features = ["sqlite-backend"] }

# RocksDB（高性能，生产就绪）
murmur = { path = "../murmur", features = ["rocksdb-backend"] }
```

### 可选 Feature

```toml
# 启用带版本控制的文件操作
murmur = { path = "../murmur", features = ["file-ops"] }
```

## 错误处理

所有操作返回 `Result<T, Error>`：

```rust
use murmur::Error;

match swarm.put_file(path).await {
    Ok(key) => println!("已上传: {}", key),
    Err(Error::FileTooLarge { size, max }) => {
        eprintln!("文件过大: {} 字节 (上限: {})", size, max);
    }
    Err(Error::VersionConflict { expected, current }) => {
        eprintln!("版本冲突: 期望 {}, 实际 {}", expected, current);
    }
    Err(Error::FileConflictLocked { file_name }) => {
        eprintln!("文件 {} 因未解决的冲突被锁定", file_name);
    }
    Err(e) => eprintln!("错误: {}", e),
}
```

## 性能

在现代笔记本上的基准测试：

- **吞吐量**: ~10,000 次操作/秒（单节点）
- **延迟**: 局域网操作 <10ms
- **内存**: ~10MB 基线 + 数据量
- **存储**: 取决于后端选择

运行基准测试: `cargo run --example benchmark --release`

## 文档

- [docs/](docs/) — 详细技术文档
- [examples/](examples/) — 可运行示例
- [ROADMAP.md](ROADMAP.md) — 项目路线图

## License

MIT OR Apache-2.0
