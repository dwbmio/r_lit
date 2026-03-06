# Murmur

> 分布式 P2P 协作库。零配置，零服务器。

## 简介

一个 Rust 库，让设备之间无需中心服务器即可同步数据。同一 WiFi 下的设备自动发现、自动同步。

基于 CRDT 实现无冲突合并，内置版本控制，支持多种存储后端。

## 为什么用这个？

- **无需服务器** — 纯 P2P，局域网直连
- **零配置** — mDNS 自动发现
- **无冲突同步** — CRDT (Automerge)
- **内置版本控制** — 数据时间旅行 (可选)
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

## 核心功能

- **P2P 网络**: iroh-net，自动 NAT 穿透，mDNS 局域网发现
- **CRDT 同步**: Automerge，多节点并发修改无冲突
- **Leader 选举**: Bully 算法，自动协调者选举
- **可插拔存储**: redb (默认)、SQLite、RocksDB

## 文件操作 (可选)

启用 `file-ops` feature 获得带版本控制的文件同步：

```toml
murmur = { path = "../murmur", features = ["file-ops"] }
```

```rust
let key = swarm.put_file(Path::new("document.txt")).await?;
swarm.get_file(&key, Path::new("output.txt")).await?;
swarm.get_file_version(&key, 3, Path::new("old.txt")).await?;
```

## 存储后端

```toml
# redb (默认，快速嵌入式)
murmur = { path = "../murmur" }

# SQLite
murmur = { path = "../murmur", features = ["sqlite-backend"] }

# RocksDB
murmur = { path = "../murmur", features = ["rocksdb-backend"] }
```

## 示例

```bash
cargo run --example basic
cargo run --example group_chat
cargo run --example file_sync --features file-ops
cargo run --example benchmark --release
```

## 文档

- [docs/](docs/) — 详细技术文档
- [examples/](examples/) — 可运行示例
- [ROADMAP.md](ROADMAP.md) — 项目路线图

## License

MIT OR Apache-2.0
