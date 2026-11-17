# Murmur 跨公网协同编辑 Roadmap

## 当前状态

### ✅ 已实现
- ✅ 基于 iroh-net 的 P2P 网络层
- ✅ 基于 Automerge 的 CRDT 同步
- ✅ 基于 redb 的本地持久化
- ✅ Bully 算法的 Leader 选举
- ✅ mDNS 局域网节点发现
- ✅ iroh relay 服务器连接（NAT 穿透）
- ✅ 单机多进程测试
- ✅ 局域网内自动发现和连接

### ❌ 待实现
- ❌ 跨公网节点发现机制
- ❌ 手动连接功能
- ❌ 邀请链接机制
- ❌ 中心化节点注册服务
- ❌ DHT 分布式节点发现

---

## 核心洞察：iroh 的跨公网能力

### iroh-net 的工作原理

iroh-net 是一个强大的 P2P 网络库，提供了完整的 NAT 穿透解决方案：

```
┌─────────────┐                    ┌─────────────┐
│   Node A    │                    │   Node B    │
│  (NAT 后)   │                    │  (NAT 后)   │
└──────┬──────┘                    └──────┬──────┘
       │                                  │
       │  1. 连接 Relay                   │
       ├──────────────┐                   │
       │              │                   │
       │              ▼                   │
       │      ┌───────────────┐           │
       │      │  Relay Server │◄──────────┤
       │      │  (公网服务器)  │           │
       │      └───────────────┘           │
       │              │                   │
       │  2. 通过 Relay 交换地址信息      │
       │◄─────────────┼──────────────────►│
       │              │                   │
       │  3. 尝试直连 (STUN/ICE)          │
       │◄─────────────────────────────────►│
       │              │                   │
       │  4. 如果直连失败，使用 Relay 转发 │
       │◄─────────────┼──────────────────►│
                      │
```

### 关键特性

1. **自动 NAT 穿透**：
   - 使用 STUN 获取公网地址
   - 使用 ICE 协议尝试打洞
   - 支持多种 NAT 类型

2. **Relay 服务器**：
   - 官方提供免费的 relay 服务器
   - 地址：`https://euw1-1.relay.iroh.network/`
   - 当直连失败时自动使用 relay 转发

3. **自动选择最优路径**：
   - 优先尝试直连（低延迟）
   - 失败后自动降级到 relay（保证连通性）

### 为什么当前无法跨公网？

**问题不在于连接，而在于发现**：

- ✅ 两个节点都能连接到 relay 服务器
- ✅ 如果知道对方的 Node ID，可以通过 relay 通信
- ❌ **但是它们不知道对方的存在**（缺少节点发现机制）

---

## 实现方案对比

### 方案 1：邀请链接机制 ⭐ 推荐

#### 原理
```
用户 A 创建群组 → 生成邀请链接 → 分享给用户 B → 用户 B 通过链接加入
```

#### 邀请链接格式
```
murmur://join?group=<group_id>&node=<node_id>&relay=<relay_url>
```

或使用 Base64 编码：
```
murmur://join/<base64_encoded_data>
```

#### 实现步骤

**Phase 1: 基础邀请链接（1-2 天）**

1. **生成邀请链接**
   ```rust
   // 在 Swarm 中添加方法
   pub async fn generate_invite_link(&self) -> Result<String> {
       let node_id = self.node_id().await;
       let node_addr = self.node_addr().await;

       let invite = InviteData {
           group_id: self.group_id.clone(),
           node_id: node_id.to_string(),
           relay_url: node_addr.relay_url.map(|u| u.to_string()),
           direct_addresses: node_addr.direct_addresses,
           timestamp: SystemTime::now(),
       };

       let encoded = base64::encode(serde_json::to_string(&invite)?);
       Ok(format!("murmur://join/{}", encoded))
   }
   ```

2. **解析邀请链接**
   ```rust
   pub async fn join_by_invite(&self, invite_link: &str) -> Result<()> {
       // 解析链接
       let invite = parse_invite_link(invite_link)?;

       // 构建 NodeAddr
       let node_addr = NodeAddr {
           node_id: PublicKey::from_str(&invite.node_id)?,
           relay_url: invite.relay_url.map(|u| RelayUrl::from_str(&u)),
           direct_addresses: invite.direct_addresses,
       };

       // 连接到节点
       let network = self.inner.network.read().await;
       network.connect(node_addr).await?;

       Ok(())
   }
   ```

3. **GUI 集成**
   - 在群组大厅页面添加"生成邀请链接"按钮
   - 显示二维码（可选）
   - 添加"通过邀请链接加入"输入框

**Phase 2: 增强功能（2-3 天）**

4. **邀请链接过期机制**
   ```rust
   pub struct InviteData {
       // ... 其他字段
       expires_at: Option<SystemTime>,
       max_uses: Option<u32>,
   }
   ```

5. **邀请链接验证**
   - 检查是否过期
   - 检查使用次数
   - 可选：添加密码保护

6. **二维码支持**
   ```rust
   use qrcode::QrCode;

   pub fn generate_qr_code(invite_link: &str) -> Result<Vec<u8>> {
       let code = QrCode::new(invite_link)?;
       let image = code.render::<Luma<u8>>().build();
       // 返回 PNG 图片数据
   }
   ```

#### 优点
- ✅ 实现简单（1-2 天）
- ✅ 用户体验好（类似 Zoom/Discord）
- ✅ 无需额外服务器
- ✅ 支持二维码分享

#### 缺点
- ❌ 需要带外通信（微信/邮件分享链接）
- ❌ 第一个节点必须在线

#### 适用场景
- 小团队协作
- 临时会议
- 一对一协作

---

### 方案 2：中心化节点注册服务

#### 原理
```
节点启动 → 向注册服务器报告 → 查询同组节点 → 建立连接
```

#### 架构
```
┌─────────────┐         ┌─────────────────────┐         ┌─────────────┐
│   Node A    │────────►│  Registry Server    │◄────────│   Node B    │
│             │  注册    │  (HTTP/WebSocket)   │  注册   │             │
└─────────────┘         └─────────────────────┘         └─────────────┘
       │                          │                             │
       │  查询同组节点              │                             │
       │◄─────────────────────────┤                             │
       │                          │                             │
       │  获取 Node B 的地址        │                             │
       │                          │                             │
       │  通过 iroh relay 直接连接                                │
       │◄────────────────────────────────────────────────────────►│
```

#### 实现步骤

**Phase 1: 简单的 HTTP 注册服务（3-4 天）**

1. **注册服务器（Rust + Axum）**
   ```rust
   // registry-server/src/main.rs

   #[derive(Serialize, Deserialize)]
   struct NodeRegistration {
       group_id: String,
       node_id: String,
       node_addr: String,  // JSON 序列化的 NodeAddr
       nickname: String,
       last_seen: SystemTime,
   }

   // POST /register
   async fn register_node(Json(reg): Json<NodeRegistration>) -> StatusCode {
       // 存储到内存或 Redis
       REGISTRY.write().await.insert(
           (reg.group_id.clone(), reg.node_id.clone()),
           reg
       );
       StatusCode::OK
   }

   // GET /discover/:group_id
   async fn discover_nodes(Path(group_id): Path<String>) -> Json<Vec<NodeRegistration>> {
       let nodes = REGISTRY.read().await
           .iter()
           .filter(|((gid, _), _)| gid == &group_id)
           .filter(|(_, node)| node.last_seen.elapsed() < Duration::from_secs(60))
           .map(|(_, node)| node.clone())
           .collect();
       Json(nodes)
   }

   // DELETE /unregister/:group_id/:node_id
   async fn unregister_node(Path((group_id, node_id)): Path<(String, String)>) {
       REGISTRY.write().await.remove(&(group_id, node_id));
   }
   ```

2. **客户端集成**
   ```rust
   // murmur/src/registry.rs

   pub struct RegistryClient {
       server_url: String,
       client: reqwest::Client,
   }

   impl RegistryClient {
       pub async fn register(&self, group_id: &str, node_info: NodeInfo) -> Result<()> {
           self.client
               .post(format!("{}/register", self.server_url))
               .json(&node_info)
               .send()
               .await?;
           Ok(())
       }

       pub async fn discover(&self, group_id: &str) -> Result<Vec<NodeInfo>> {
           let resp = self.client
               .get(format!("{}/discover/{}", self.server_url, group_id))
               .send()
               .await?;
           Ok(resp.json().await?)
       }
   }
   ```

3. **Swarm 集成**
   ```rust
   // 在 Swarm::start() 中
   pub async fn start(&self) -> Result<()> {
       // ... 现有代码

       // 如果配置了注册服务器，则注册
       if let Some(registry_url) = &self.config.registry_url {
           let registry = RegistryClient::new(registry_url);

           // 注册自己
           registry.register(&self.group_id, self.node_info()).await?;

           // 定期心跳
           self.spawn_heartbeat_to_registry(registry);

           // 发现其他节点
           let nodes = registry.discover(&self.group_id).await?;
           for node in nodes {
               self.connect_to_node(node).await?;
           }
       }

       Ok(())
   }
   ```

**Phase 2: 增强功能（2-3 天）**

4. **WebSocket 实时通知**
   - 新节点加入时推送通知
   - 节点离线时推送通知

5. **持久化存储**
   - 使用 PostgreSQL/Redis 替代内存存储
   - 支持节点历史记录

6. **认证和授权**
   - API Key 认证
   - 群组访问控制

#### 优点
- ✅ 完全自动化
- ✅ 支持大规模部署
- ✅ 可以添加更多功能（统计、监控）

#### 缺点
- ❌ 需要运维服务器
- ❌ 单点故障风险
- ❌ 隐私问题（服务器知道所有节点）

#### 适用场景
- 企业内部部署
- 大规模协作
- 需要中心化管理

---

### 方案 3：DHT 分布式节点发现

#### 原理
```
使用 Kademlia DHT 在 P2P 网络中发布和查找节点
```

#### 架构
```
┌─────────────┐         ┌─────────────┐         ┌─────────────┐
│   Node A    │◄───────►│   Node C    │◄───────►│   Node B    │
│             │  DHT    │  (Bootstrap) │  DHT    │             │
└─────────────┘         └─────────────┘         └─────────────┘
       │                                                │
       │  PUT: group_id → node_id                      │
       │  GET: group_id → [node_ids]                   │
       │◄──────────────────────────────────────────────►│
```

#### 实现步骤

**Phase 1: 集成 libp2p Kademlia（5-7 天）**

1. **添加 libp2p 依赖**
   ```toml
   [dependencies]
   libp2p = { version = "0.53", features = ["kad", "tcp", "noise", "yamux"] }
   ```

2. **创建 DHT 层**
   ```rust
   // murmur/src/dht.rs

   use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent};

   pub struct DhtDiscovery {
       swarm: libp2p::Swarm<Behaviour>,
       group_id: String,
   }

   impl DhtDiscovery {
       pub async fn publish_node(&mut self, node_id: &str) -> Result<()> {
           let key = format!("murmur:group:{}", self.group_id);
           self.swarm.behaviour_mut().kad.put_record(
               Record::new(key, node_id.as_bytes().to_vec()),
               Quorum::One,
           )?;
           Ok(())
       }

       pub async fn discover_nodes(&mut self) -> Result<Vec<String>> {
           let key = format!("murmur:group:{}", self.group_id);
           self.swarm.behaviour_mut().kad.get_record(key.into());
           // 等待结果...
       }
   }
   ```

3. **与 iroh 集成**
   - 使用 DHT 发现节点 ID
   - 使用 iroh 建立实际连接

#### 优点
- ✅ 完全去中心化
- ✅ 无需额外服务器
- ✅ 高可用性

#### 缺点
- ❌ 实现复杂（5-7 天）
- ❌ 需要 Bootstrap 节点
- ❌ 发现速度较慢（几秒到几十秒）

#### 适用场景
- 完全去中心化应用
- 长期运行的网络
- 对隐私要求高

---

## 推荐实现路径

### 短期（1-2 周）：邀请链接机制 ⭐

**优先级：P0**

1. **Week 1**：
   - Day 1-2: 实现邀请链接生成和解析
   - Day 3-4: 实现手动连接功能
   - Day 5: GUI 集成和测试

2. **Week 2**：
   - Day 1-2: 添加二维码支持
   - Day 3-4: 添加过期和验证机制
   - Day 5: 完整测试和文档

**交付物**：
- ✅ 用户可以生成邀请链接
- ✅ 用户可以通过链接加入群组
- ✅ 支持二维码分享
- ✅ 跨公网协同编辑可用

### 中期（1-2 月）：中心化注册服务

**优先级：P1**

1. **Month 1**：
   - Week 1-2: 实现注册服务器
   - Week 3: 客户端集成
   - Week 4: 测试和部署

2. **Month 2**：
   - Week 1-2: WebSocket 实时通知
   - Week 3: 认证和授权
   - Week 4: 监控和运维

**交付物**：
- ✅ 自动节点发现
- ✅ 企业级部署方案
- ✅ 管理后台

### 长期（3-6 月）：DHT 分布式发现

**优先级：P2**

1. **Month 1-2**：
   - 研究和原型
   - libp2p 集成

2. **Month 3-4**：
   - 完整实现
   - 与现有系统集成

3. **Month 5-6**：
   - 性能优化
   - 大规模测试

**交付物**：
- ✅ 完全去中心化
- ✅ 无需任何服务器

---

## 技术细节

### iroh-net 的 NodeAddr 结构

```rust
pub struct NodeAddr {
    /// 节点的公钥 ID
    pub node_id: PublicKey,

    /// Relay 服务器 URL（用于 NAT 穿透）
    pub relay_url: Option<RelayUrl>,

    /// 直连地址列表（IP:Port）
    pub direct_addresses: BTreeSet<SocketAddr>,
}
```

### 连接建立流程

```rust
// 1. 创建 NodeAddr
let node_addr = NodeAddr {
    node_id: PublicKey::from_str("abc123...")?,
    relay_url: Some(RelayUrl::from_str("https://euw1-1.relay.iroh.network./")?),
    direct_addresses: BTreeSet::new(),
};

// 2. 连接（iroh 自动处理 NAT 穿透）
let conn = endpoint.connect(node_addr, b"murmur").await?;

// 3. 发送数据
let mut send = conn.open_uni().await?;
send.write_all(&data).await?;
send.finish().await?;
```

### 关键代码位置

- **网络层**：`crates/murmur/src/network.rs`
- **连接方法**：`Network::connect()`
- **节点发现**：`crates/murmur/src/discovery.rs`
- **Swarm API**：`crates/murmur/src/lib.rs`

---

## 配置示例

### 邀请链接模式（默认）

```toml
[murmur]
discovery_mode = "invite"  # 使用邀请链接
```

### 注册服务器模式

```toml
[murmur]
discovery_mode = "registry"
registry_url = "https://registry.example.com"
registry_api_key = "your-api-key"
```

### DHT 模式

```toml
[murmur]
discovery_mode = "dht"
bootstrap_nodes = [
    "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ"
]
```

---

## 测试计划

### 邀请链接测试

1. **单元测试**：
   - 链接生成和解析
   - 过期验证
   - 格式验证

2. **集成测试**：
   - 本地两个进程
   - 局域网两台机器
   - 跨公网两台机器

3. **压力测试**：
   - 100 个节点同时加入
   - 链接并发生成

### 注册服务器测试

1. **API 测试**：
   - 注册/注销
   - 发现
   - 心跳

2. **性能测试**：
   - 1000 节点注册
   - 并发查询
   - 数据库性能

3. **可靠性测试**：
   - 服务器重启
   - 网络中断
   - 数据一致性

---

## 依赖和资源

### 开发依赖

```toml
# 邀请链接
base64 = "0.21"
qrcode = "0.13"  # 可选：二维码

# 注册服务器
axum = "0.7"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
reqwest = { version = "0.11", features = ["json"] }

# DHT（可选）
libp2p = { version = "0.53", features = ["kad"] }
```

### 外部服务

- **iroh relay**：免费，官方提供
- **注册服务器**：需要自己部署（可选）
- **Bootstrap 节点**：DHT 需要（可选）

---

## 参考资料

### iroh 文档
- [iroh-net GitHub](https://github.com/n0-computer/iroh)
- [iroh 文档](https://docs.rs/iroh-net/)
- [NAT 穿透原理](https://tailscale.com/blog/how-nat-traversal-works/)

### 类似项目
- **Tailscale**：基于 WireGuard 的 P2P VPN
- **Syncthing**：P2P 文件同步
- **IPFS**：使用 libp2p 的分布式文件系统

### 协议和标准
- **STUN/TURN**：NAT 穿透标准
- **ICE**：交互式连接建立
- **Kademlia DHT**：分布式哈希表

---

## 总结

### 核心发现

**iroh-net 已经解决了跨公网连接的所有技术难题**：
- ✅ NAT 穿透
- ✅ Relay 转发
- ✅ 自动路径选择

**我们只需要解决节点发现问题**：
- 方案 1：邀请链接（最简单，1-2 周）
- 方案 2：注册服务器（最实用，1-2 月）
- 方案 3：DHT（最去中心化，3-6 月）

### 下一步行动

1. **立即**：完成局域网测试，验证基础功能
2. **本周**：实现邀请链接机制
3. **下周**：跨公网测试
4. **本月**：完善 GUI 集成
5. **下月**：考虑注册服务器（如果需要）

---

**最后更新**：2026-02-28
**维护者**：Murmur Team
