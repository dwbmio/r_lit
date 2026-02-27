# Murmur - 项目总结与路线图

## 🎯 设计初衷

### 核心理念

**"去中心化的协作存储，像使用本地数据库一样简单"**

Murmur 旨在解决分布式协作场景中的核心痛点：

1. **无需中心服务器**
   - 传统方案：需要部署 Redis/PostgreSQL/MongoDB 等中心服务器
   - Murmur：每个节点平等，P2P 直连，无单点故障

2. **自动冲突解决**
   - 传统方案：需要手动处理并发冲突（锁、版本号、重试）
   - Murmur：CRDT 自动合并，开发者无需关心冲突

3. **离线可用**
   - 传统方案：离线无法工作，必须在线
   - Murmur：本地完整副本，离线照常工作，上线自动同步

4. **简单易用**
   - 传统方案：复杂的分布式系统概念（Raft、Paxos、一致性哈希）
   - Murmur：像 SQLite 一样简单，`put/get/delete` 三个 API

### 目标场景

```
✅ 小团队协作工具（<50 人）
✅ 实时协作应用（文档、白板、聊天）
✅ 边缘计算/IoT 设备同步
✅ 游戏状态同步
✅ 本地优先应用（Local-First Software）
```

### 非目标场景

```
❌ 大规模分布式系统（>1000 节点）
❌ 强一致性要求（金融交易）
❌ 低延迟要求（<10ms）
❌ 中心化架构
```

## 🏗️ 架构设计

### 分层架构

```
┌─────────────────────────────────────────────────┐
│  应用层 API                                      │
│  - Swarm::put/get/delete                        │
│  - 简单、同步的接口                              │
├─────────────────────────────────────────────────┤
│  协调层                                          │
│  - Leader Election (Bully 算法)                 │
│  - 角色管理 (Leader/Follower)                   │
│  - 心跳检测                                      │
├─────────────────────────────────────────────────┤
│  同步层                                          │
│  - CRDT (Automerge)                             │
│  - Vector Clock (因果追踪)                      │
│  - Sequence Number (消息去重)                   │
│  - ACK 机制 (可靠送达)                          │
├─────────────────────────────────────────────────┤
│  网络层                                          │
│  - P2P (iroh)                                   │
│  - NAT 穿透                                      │
│  - Relay 中继                                    │
│  - QUIC 传输                                     │
├─────────────────────────────────────────────────┤
│  存储层                                          │
│  - redb (默认，纯 Rust KV)                      │
│  - SQLite (可选，关系型)                        │
│  - RocksDB (可选，高性能)                       │
└─────────────────────────────────────────────────┘
```

### 核心技术选型

| 组件 | 技术 | 理由 |
|------|------|------|
| **P2P 网络** | iroh | NAT 穿透、Relay 自动选择 |
| **CRDT** | Automerge | 成熟、自动冲突解决 |
| **因果追踪** | Vector Clock | 检测并发、保证顺序 |
| **存储** | redb | 纯 Rust、高性能、零依赖 |
| **选举** | Bully 算法 | 简单、去中心化 |

## ✅ 已实现功能

### 1. 核心功能

#### 1.1 P2P 网络层
```rust
✅ iroh 端点初始化
✅ NAT 穿透（自动 Relay）
✅ 节点连接管理
✅ 消息序列化/反序列化
✅ 广播机制
✅ 群组隔离（group_id）
```

#### 1.2 CRDT 同步
```rust
✅ Automerge 集成
✅ 操作生成（put/delete）
✅ 变更应用（apply_changes）
✅ 文档合并（merge）
✅ 幂等性保证
```

#### 1.3 因果一致性
```rust
✅ Vector Clock 实现
✅ 因果关系追踪
✅ 并发检测
✅ 消息序列号
✅ ACK 确认机制
```

#### 1.4 Leader 选举
```rust
✅ Bully 算法实现
✅ 自动选举
✅ 心跳机制（2s 间隔）
✅ 故障检测（5s 超时）
✅ 自动重新选举
```

#### 1.5 存储后端
```rust
✅ 存储抽象层（StorageBackend trait）
✅ redb 后端（默认）
✅ SQLite 后端（可选）
✅ RocksDB 后端（可选）
✅ Feature flags 切换
```

#### 1.6 API 设计
```rust
✅ Swarm::builder() 构建器模式
✅ put/get/delete 简单 API
✅ is_leader/leader_id 角色查询
✅ connect_peer 手动连接
✅ connected_peers 节点列表
✅ shutdown 优雅关闭
```

### 2. 示例程序

```
✅ examples/basic.rs - 基础使用
✅ examples/group_chat.rs - 群组聊天
✅ examples/shared_kv.rs - 共享配置管理
```

### 3. 文档

```
✅ README.md - 项目介绍
✅ BROADCAST.md - 广播机制说明
✅ INTEGRITY.md - 数据完整性保证
✅ STORAGE_BACKENDS.md - 存储后端对比
✅ examples/SHARED_KV.md - 示例说明
✅ CLAUDE.md - 项目指南
```

## 🚧 未实现功能

### 1. 网络层增强

#### 1.1 自动发现
```rust
❌ mDNS 本地网络发现
❌ DHT 全局节点发现
❌ Rendezvous 服务器
❌ 持久化 peer 列表
```

**优先级：高**
**原因：** 当前需要手动交换节点地址，用户体验差

**实现方案：**
```rust
// mDNS 本地发现
swarm.enable_mdns_discovery().await?;

// DHT 全局发现
swarm.enable_dht_discovery().await?;

// 自动连接到发现的节点
swarm.auto_connect(true);
```

#### 1.2 连接管理
```rust
❌ 连接池管理
❌ 自动重连
❌ 连接质量监控
❌ 带宽限制
```

**优先级：中**

### 2. 可靠性增强

#### 2.1 消息重传
```rust
❌ 超时重传队列
❌ 指数退避
❌ 最大重试次数
❌ 死信队列
```

**优先级：高**
**原因：** 当前只有 ACK，没有重传，消息可能丢失

**实现方案：**
```rust
struct RetransmissionQueue {
    pending: HashMap<u64, (Message, Instant, u32)>,  // seq_num -> (msg, sent_at, retry_count)
    timeout: Duration,
    max_retries: u32,
}

impl RetransmissionQueue {
    async fn check_timeouts(&mut self, network: &Network) {
        for (seq_num, (msg, sent_at, retry_count)) in &mut self.pending {
            if sent_at.elapsed() > self.timeout {
                if *retry_count < self.max_retries {
                    network.broadcast(msg.clone()).await?;
                    *sent_at = Instant::now();
                    *retry_count += 1;
                } else {
                    // 移到死信队列
                    warn!("Message {} failed after {} retries", seq_num, retry_count);
                }
            }
        }
    }
}
```

#### 2.2 消息排序
```rust
❌ 乱序消息缓冲
❌ 按序交付
❌ 缺失消息检测
❌ 请求重传
```

**优先级：中**

### 3. 性能优化

#### 3.1 批量操作
```rust
❌ 批量 put/get
❌ 事务支持
❌ 批量同步
```

**优先级：中**

**实现方案：**
```rust
// 批量写入
swarm.batch_put(vec![
    ("key1", b"value1"),
    ("key2", b"value2"),
]).await?;

// 事务
let tx = swarm.begin_transaction().await?;
tx.put("key1", b"value1")?;
tx.put("key2", b"value2")?;
tx.commit().await?;
```

#### 3.2 压缩与优化
```rust
❌ 消息压缩（zstd/lz4）
❌ 增量同步
❌ 向量时钟压缩
❌ 垃圾回收
```

**优先级：低**

### 4. 高级特性

#### 4.1 数据类型
```rust
❌ 结构化数据（JSON/MessagePack）
❌ 列表/集合/映射
❌ 计数器（CRDT Counter）
❌ 寄存器（LWW-Register）
```

**优先级：中**

**实现方案：**
```rust
// 结构化数据
#[derive(Serialize, Deserialize)]
struct User {
    name: String,
    age: u32,
}

swarm.put_json("user:123", &User { name: "Alice", age: 30 }).await?;
let user: User = swarm.get_json("user:123").await?;

// CRDT 计数器
swarm.counter_increment("likes", 1).await?;
let count = swarm.counter_get("likes").await?;
```

#### 4.2 查询能力
```rust
❌ 前缀查询
❌ 范围查询
❌ 模糊匹配
❌ 二级索引
```

**优先级：低**

#### 4.3 安全性
```rust
❌ 端到端加密
❌ 节点认证
❌ 权限控制
❌ 审计日志
```

**优先级：高**（生产环境必需）

**实现方案：**
```rust
// 加密
swarm.enable_encryption(keypair).await?;

// 认证
swarm.require_authentication(auth_provider).await?;

// 权限
swarm.set_acl("key", vec!["alice", "bob"]).await?;
```

#### 4.4 监控与调试
```rust
❌ Metrics 导出（Prometheus）
❌ 分布式追踪（OpenTelemetry）
❌ 调试 UI
❌ 性能分析
```

**优先级：中**

### 5. 关系型支持

#### 5.1 SQL 查询层
```rust
❌ Gluesql 集成
❌ SQL 查询接口
❌ JOIN/聚合
❌ 索引
```

**优先级：中**

**实现方案：**
```rust
#[cfg(feature = "relational")]
{
    // SQL 查询
    let results = swarm.sql("
        SELECT user_id, COUNT(*) as msg_count
        FROM messages
        WHERE timestamp > ?
        GROUP BY user_id
        ORDER BY msg_count DESC
        LIMIT 10
    ", &[timestamp]).await?;

    // 自动同步 KV → SQL
    swarm.enable_sql_sync().await?;
}
```

## 🗺️ Roadmap

### Phase 1: 核心稳定性 ✅ (已完成)

**目标：** 基础功能可用，API 稳定

- ✅ P2P 网络层
- ✅ CRDT 同步
- ✅ Leader 选举
- ✅ 存储后端
- ✅ 基础示例

**时间：** 已完成

### Phase 2: 可靠性增强 🚧 (进行中)

**目标：** 生产可用，消息不丢失

- ✅ Vector Clock
- ✅ Sequence Number
- ✅ ACK 机制
- 🚧 消息重传
- 🚧 乱序处理
- 🚧 自动发现（mDNS）

**时间：** 2-3 周

**优先级：**
1. 消息重传（高）
2. mDNS 发现（高）
3. 乱序处理（中）

### Phase 3: 性能优化 📅 (计划中)

**目标：** 支持更大规模，更高吞吐

- 📅 批量操作
- 📅 消息压缩
- 📅 增量同步
- 📅 向量时钟压缩
- 📅 性能测试套件

**时间：** 1-2 个月

### Phase 4: 高级特性 📅 (计划中)

**目标：** 功能丰富，易用性提升

- 📅 结构化数据
- 📅 CRDT 数据类型（Counter/Set/List）
- 📅 查询能力（前缀/范围）
- 📅 关系型支持（Gluesql）

**时间：** 2-3 个月

### Phase 5: 生产就绪 📅 (未来)

**目标：** 企业级可用

- 📅 端到端加密
- 📅 节点认证
- 📅 权限控制
- 📅 监控与追踪
- 📅 性能调优
- 📅 文档完善

**时间：** 3-6 个月

## 📊 功能矩阵

| 功能 | 状态 | 优先级 | 预计时间 |
|------|------|--------|----------|
| **核心功能** |
| P2P 网络 | ✅ 完成 | - | - |
| CRDT 同步 | ✅ 完成 | - | - |
| Leader 选举 | ✅ 完成 | - | - |
| 存储后端 | ✅ 完成 | - | - |
| **可靠性** |
| Vector Clock | ✅ 完成 | - | - |
| 消息重传 | 🚧 进行中 | 高 | 1 周 |
| mDNS 发现 | 📅 计划 | 高 | 1 周 |
| 乱序处理 | 📅 计划 | 中 | 2 周 |
| **性能** |
| 批量操作 | 📅 计划 | 中 | 2 周 |
| 消息压缩 | 📅 计划 | 低 | 1 周 |
| 增量同步 | 📅 计划 | 中 | 2 周 |
| **高级特性** |
| 结构化数据 | 📅 计划 | 中 | 2 周 |
| CRDT 类型 | 📅 计划 | 中 | 3 周 |
| 查询能力 | 📅 计划 | 低 | 2 周 |
| 关系型支持 | 📅 计划 | 中 | 3 周 |
| **安全性** |
| 加密 | 📅 计划 | 高 | 2 周 |
| 认证 | 📅 计划 | 高 | 2 周 |
| 权限控制 | 📅 计划 | 中 | 2 周 |
| **监控** |
| Metrics | 📅 计划 | 中 | 1 周 |
| 追踪 | 📅 计划 | 低 | 1 周 |
| 调试 UI | 📅 计划 | 低 | 3 周 |

## 🎯 近期目标（2 周内）

### 1. 消息重传机制
```rust
// 实现可靠送达
- 重传队列
- 超时检测
- 指数退避
- 死信队列
```

### 2. mDNS 自动发现
```rust
// 局域网自动发现
- mDNS 广播
- 自动连接
- 节点列表维护
```

### 3. 示例完善
```rust
// 更多实际场景
- 协作文档编辑
- 实时聊天室
- 分布式任务队列
```

## 💭 设计权衡

### 1. 最终一致性 vs 强一致性

**选择：** 最终一致性（CRDT）

**理由：**
- ✅ 去中心化
- ✅ 离线可用
- ✅ 高可用
- ❌ 不适合金融场景

### 2. P2P vs 客户端-服务器

**选择：** P2P（iroh）

**理由：**
- ✅ 无单点故障
- ✅ 无需部署服务器
- ✅ 边缘计算友好
- ❌ 连接管理复杂

### 3. 纯 Rust vs C/C++ 绑定

**选择：** 纯 Rust（redb 默认）

**理由：**
- ✅ 编译简单
- ✅ 跨平台
- ✅ 静态链接
- ❌ 性能略低于 RocksDB

### 4. 简单 API vs 功能丰富

**选择：** 简单 API（put/get/delete）

**理由：**
- ✅ 易学易用
- ✅ 降低门槛
- ✅ 渐进增强
- ❌ 高级功能需要扩展

## 🌟 核心价值

1. **简单**：像 SQLite 一样简单
2. **去中心化**：无需服务器
3. **自动同步**：CRDT 自动解决冲突
4. **离线可用**：本地完整副本
5. **纯 Rust**：安全、高性能、跨平台

## 📈 成功指标

- ✅ API 稳定（无破坏性变更）
- 🚧 消息送达率 >99.9%
- 📅 支持 100 节点
- 📅 吞吐量 >10K ops/s
- 📅 延迟 <100ms (P99)
- 📅 10+ 生产用户

## 🙏 致谢

- **iroh**：P2P 网络层
- **Automerge**：CRDT 实现
- **redb**：纯 Rust 存储
- **TiKV**：分布式系统灵感

---

**Murmur - 让分布式协作像本地数据库一样简单**
