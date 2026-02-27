# Murmur - 群组广播机制说明

## 🎯 广播工作原理

### 1. 群组隔离

每个 Swarm 实例创建时指定一个 `group_id`：

```rust
let swarm = Swarm::builder()
    .group_id("my-chat-room")  // 群组 ID
    .build()
    .await?;
```

**只有相同 `group_id` 的节点才会互相通信。**

### 2. 节点连接

Murmur 使用 **手动连接** 模式（类似 WebRTC）：

```
┌─────────┐                    ┌─────────┐
│ Alice   │ ←─── connect ────→ │  Bob    │
│ (Node1) │                    │ (Node2) │
└─────────┘                    └─────────┘
     ↓                              ↓
     └──────── connect ─────────────┘
                  ↓
            ┌─────────┐
            │ Charlie │
            │ (Node3) │
            └─────────┘
```

**连接步骤：**

1. **Alice 启动**：
   ```bash
   cargo run --example group_chat alice
   # 输出：Node Address: <alice-addr>
   ```

2. **Bob 连接 Alice**：
   ```bash
   cargo run --example group_chat bob <alice-addr>
   ```

3. **Charlie 连接任意节点**（Alice 或 Bob）：
   ```bash
   cargo run --example group_chat charlie <bob-addr>
   ```

### 3. 广播机制

当任意节点调用 `swarm.put(key, value)` 时：

```rust
// Alice 发送消息
swarm.put("msg:alice", b"Hello everyone!").await?;
```

**内部流程：**

```
1. Alice 本地存储
   ├─ SQLite: 写入 kv_store 表
   └─ CRDT: Automerge 记录变更

2. 生成 CRDT 变更操作
   └─ changes = sync.put(key, value)

3. 广播给所有连接的节点
   ├─ network.broadcast(CrdtUpdate { key, operation: changes })
   └─ 遍历 peers HashMap，逐个发送

4. Bob 和 Charlie 收到消息
   ├─ 应用 CRDT 变更: sync.apply_changes(operation)
   ├─ 写入本地 SQLite
   └─ 自动解决冲突（Automerge CRDT）
```

### 4. 实际代码示例

```rust
use murmur::Swarm;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 创建群组
    let swarm = Swarm::builder()
        .storage_path("./data/alice")
        .group_id("my-chat-room")
        .build()
        .await?;

    swarm.start().await?;

    // 获取自己的地址（分享给其他人）
    println!("My address: {}", swarm.node_addr().await);

    // 连接其他节点（如果有）
    if let Some(peer_addr) = std::env::args().nth(2) {
        swarm.connect_peer(&peer_addr).await?;
    }

    // 发送消息（自动广播）
    swarm.put("msg:alice", b"Hello!").await?;

    // 读取消息（从本地 CRDT）
    if let Some(msg) = swarm.get("msg:bob").await? {
        println!("Bob says: {}", String::from_utf8_lossy(&msg));
    }

    Ok(())
}
```

## 🔄 同步保证

### CRDT 特性

- **最终一致性**：所有节点最终会达到相同状态
- **无冲突合并**：Automerge 自动解决并发修改
- **离线可用**：节点离线时仍可本地操作，上线后自动同步

### 网络拓扑

```
完全连接（Full Mesh）：
每个节点都与其他节点直接连接

Alice ←→ Bob
  ↓  ×  ↓
Charlie ←→ Dave

优点：
✓ 低延迟（直接通信）
✓ 高可用（无单点故障）
✓ 简单实现

缺点：
✗ 连接数 = N*(N-1)/2
✗ 不适合大规模（>50 节点）
```

## 🚀 运行示例

### 终端 1 - Alice
```bash
cd murmur
cargo run --example group_chat alice
# 复制输出的 Node Address
```

### 终端 2 - Bob
```bash
cargo run --example group_chat bob <alice-address>
```

### 终端 3 - Charlie
```bash
cargo run --example group_chat charlie <alice-address>
```

**观察输出：**
- 每个节点会显示连接的 peers
- 消息会自动同步到所有节点
- Leader 选举结果（ID 最大的成为 Leader）

## ⚠️ 当前限制

1. **手动连接**：需要手动交换节点地址（未来可添加 mDNS 自动发现）
2. **入站连接限制**：iroh 0.28 API 限制，入站连接无法获取 peer_id
3. **小规模网络**：适合 <50 节点的群组
4. **无持久化连接列表**：重启后需要重新连接

## 🔮 未来改进

- [ ] mDNS 本地网络自动发现
- [ ] DHT 全局节点发现
- [ ] 持久化 peer 列表
- [ ] 升级到最新 iroh 版本
- [ ] 添加消息历史同步
- [ ] 实现消息删除/编辑
