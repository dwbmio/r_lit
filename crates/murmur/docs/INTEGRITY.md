# Murmur - 数据完整性与时序保证

## 🎯 保证机制总览

### ✅ 已实现的保证

| 层级 | 机制 | 保证内容 |
|------|------|----------|
| **CRDT 层** | Automerge | 因果一致性、幂等性、收敛性 |
| **网络层** | 向量时钟 | 因果顺序追踪 |
| **网络层** | 序列号 | 消息去重、顺序检测 |
| **网络层** | ACK 机制 | 消息送达确认 |
| **存储层** | SQLite | 原子性、持久化 |

## 📊 详细说明

### 1. CRDT 层保证（Automerge）

**因果一致性（Causal Consistency）**
```rust
// Automerge 内部维护操作的因果关系
// 例如：Alice 的操作 A happens-before Bob 的操作 B
// 所有节点都会以相同的因果顺序应用操作

Alice: put("counter", 1)  →  put("counter", 2)
                                      ↓
Bob:                          get("counter") = 2  // 保证看到 1 → 2 的顺序
```

**幂等性（Idempotence）**
```rust
// 同一个 CRDT 变更可以多次应用，结果相同
let changes = sync.put("key", b"value")?;

sync.apply_changes(&changes)?;  // 第一次
sync.apply_changes(&changes)?;  // 第二次 - 无副作用
sync.apply_changes(&changes)?;  // 第三次 - 仍然无副作用
```

**收敛性（Convergence）**
```rust
// 所有节点最终会达到相同状态，无论操作到达顺序
Node A: put("x", "1") → put("y", "2")
Node B: put("y", "2") → put("x", "1")  // 不同顺序

// 最终状态相同：{ x: "1", y: "2" }
```

### 2. 向量时钟（Vector Clock）

**追踪因果关系**
```rust
// 每个节点维护一个向量时钟
VectorClock {
    "alice": 5,   // Alice 发送了 5 条消息
    "bob": 3,     // Bob 发送了 3 条消息
    "charlie": 2  // Charlie 发送了 2 条消息
}

// 发送消息时：
1. 递增自己的时钟
2. 附带当前向量时钟

// 接收消息时：
1. 合并向量时钟（取每个分量的最大值）
2. 递增自己的时钟
```

**检测并发操作**
```rust
// Alice 和 Bob 同时修改同一个 key
Alice: VC{alice:1, bob:0} → put("status", "online")
Bob:   VC{alice:0, bob:1} → put("status", "busy")

// 这两个操作是并发的（concurrent）
// CRDT 会自动解决冲突（例如：Last-Write-Wins）
```

**示例代码**
```rust
// 发送消息
let seq_num = network.next_seq_num().await;
let vector_clock = network.get_vector_clock().await;

let message = Message::CrdtUpdate {
    key: "user:alice".to_string(),
    operation: changes,
    seq_num,           // 序列号：1, 2, 3, ...
    vector_clock,      // VC{alice:5, bob:3}
};

network.broadcast(message).await?;
network.increment_vector_clock().await;  // alice: 5 → 6

// 接收消息
network.merge_vector_clock(&received_vc).await;  // 合并时钟
```

### 3. 序列号（Sequence Number）

**消息去重**
```rust
// 每个节点维护已接收的序列号集合
received_seqs: HashSet<(NodeId, u64)>

// 接收消息时检查
if received_seqs.contains(&(peer_id, seq_num)) {
    // 重复消息，丢弃
    return;
}

received_seqs.insert((peer_id, seq_num));
```

**顺序检测**
```rust
// 检测消息乱序
expected_seq: HashMap<NodeId, u64>

if seq_num != expected_seq[peer_id] + 1 {
    warn!("Out-of-order message: expected {}, got {}",
          expected_seq[peer_id] + 1, seq_num);
    // CRDT 仍然可以正确处理，但可以记录日志
}
```

### 4. ACK 机制（Acknowledgment）

**可靠送达**
```rust
// 发送方
1. 发送消息
2. 启动超时计时器（例如 5 秒）
3. 等待 ACK

// 接收方
1. 接收消息
2. 处理消息
3. 发送 ACK

// 超时重传
if !received_ack_within_timeout {
    retransmit_message();
}
```

**当前实现**
```rust
// 接收方发送 ACK
let ack_msg = Message::Ack { seq_num };
network.send(&peer_id.to_string(), ack_msg).await?;

// 发送方接收 ACK
Message::Ack { seq_num } => {
    debug!("Received ACK for seq={}", seq_num);
    // TODO: 从重传队列中移除
}
```

### 5. 存储层保证（SQLite）

**原子性**
```sql
-- 单个写操作是原子的
INSERT OR REPLACE INTO kv_store (key, value, version, updated_at)
VALUES ('user:alice', 'online', 1, 1234567890);
```

**时间戳**
```rust
// 记录更新时间
updated_at: i64  // Unix timestamp

// 可用于：
// - 调试（查看最后更新时间）
// - 冲突解决（Last-Write-Wins）
// - 数据过期（TTL）
```

## 🔄 完整流程示例

### 场景：Alice 和 Bob 同时修改同一个 key

```
时间线：

T1: Alice: put("status", "online")
    ├─ VC{alice:1, bob:0}
    ├─ seq_num: 1
    └─ 广播给 Bob

T2: Bob: put("status", "busy")
    ├─ VC{alice:0, bob:1}
    ├─ seq_num: 1
    └─ 广播给 Alice

T3: Alice 收到 Bob 的消息
    ├─ 合并 VC: {alice:1, bob:1} → {alice:2, bob:1}
    ├─ 检测并发：VC{alice:1, bob:0} 和 VC{alice:0, bob:1} 是并发的
    ├─ CRDT 自动解决冲突（例如：Bob 的值胜出）
    ├─ 最终状态：status = "busy"
    └─ 发送 ACK

T4: Bob 收到 Alice 的消息
    ├─ 合并 VC: {alice:1, bob:1} → {alice:1, bob:2}
    ├─ CRDT 应用相同的冲突解决策略
    ├─ 最终状态：status = "busy"
    └─ 发送 ACK

结果：两个节点收敛到相同状态
```

## ⚠️ 当前限制

### 1. 未实现的功能

- ❌ **重传队列**：ACK 超时后的自动重传
- ❌ **消息缓冲**：乱序消息的重排序
- ❌ **持久化向量时钟**：重启后丢失因果信息
- ❌ **垃圾回收**：旧序列号的清理

### 2. 性能考虑

**向量时钟大小**
```
空间复杂度：O(N)，N = 节点数
每条消息携带：N * 8 字节（假设 u64）

例如：
- 10 节点：80 字节
- 100 节点：800 字节
- 1000 节点：8KB  ⚠️ 开始影响性能
```

**优化方案**
- 使用版本向量压缩（Version Vector Compression）
- 定期清理不活跃节点的时钟
- 使用混合逻辑时钟（Hybrid Logical Clock）

### 3. 网络分区

**脑裂场景**
```
网络分区前：
Alice ←→ Bob ←→ Charlie

网络分区后：
Alice ←→ Bob    |    Charlie (孤立)

问题：
- Charlie 无法接收 Alice/Bob 的更新
- Charlie 的更新无法传播

恢复后：
- CRDT 自动合并所有变更
- 向量时钟检测分区期间的并发操作
```

## 🚀 使用建议

### 1. 检测冲突

```rust
// 在应用层检测并发修改
let old_vc = get_stored_vector_clock(&key);
let new_vc = message.vector_clock;

if old_vc.is_concurrent(&new_vc) {
    warn!("Concurrent modification detected for key: {}", key);
    // 记录日志或通知用户
}
```

### 2. 监控消息延迟

```rust
// 记录消息发送时间
let send_time = SystemTime::now();

// 接收时计算延迟
let latency = SystemTime::now().duration_since(send_time)?;
if latency > Duration::from_secs(5) {
    warn!("High latency detected: {:?}", latency);
}
```

### 3. 处理网络分区

```rust
// 定期检查节点连通性
if last_heartbeat.elapsed() > Duration::from_secs(30) {
    warn!("Node {} may be partitioned", peer_id);
    // 触发重连或通知用户
}
```

## 📚 参考资料

- [Automerge CRDT](https://automerge.org/)
- [Vector Clocks](https://en.wikipedia.org/wiki/Vector_clock)
- [Lamport Timestamps](https://en.wikipedia.org/wiki/Lamport_timestamp)
- [Conflict-free Replicated Data Types](https://crdt.tech/)
