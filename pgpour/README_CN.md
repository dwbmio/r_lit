# pgpour

Postgres CDC → Kafka 实时数据管道。基于 [supabase/etl](https://github.com/supabase/etl) 构建，独立于业务服务运行，纯运维侧部署。

## 架构

```
┌────────────┐  logical replication  ┌──────────┐  produce  ┌───────────────┐
│ PostgreSQL │ ────────────────────→ │ pgpour   │ ────────→ │ Topic-A (tbl1)│
│ (WAL)      │                       │          │ ────────→ │ Topic-B (tbl2)│
└────────────┘                       └──────────┘           └───────────────┘
```

- PG 端零侵入（仅需 `wal_level=logical` + `CREATE PUBLICATION`）
- 每张表对应一个 Kafka topic: `{prefix}.{schema}.{table}`
- 消息格式: JSON envelope (`op` / `before` / `after`)
- 多 topic 扇出：一个 replication slot，通过 `kafka.targets` 独立路由事件到不同 topic
- 批量投递 + lz4 压缩，高吞吐低 CPU 开销

## 前置条件

- Rust >= 1.90.0, cmake
- PostgreSQL 14+ 且 `wal_level = logical`
- 连接用户需 `REPLICATION` 权限

## 快速开始

### 1. PG 端配置

```sql
-- 1. 确认 wal_level（需 superuser，修改后重启 PG）
SHOW wal_level;  -- 期望输出: logical
-- ALTER SYSTEM SET wal_level = logical;  -- 然后重启 PG

-- 2. 创建 publication
CREATE PUBLICATION cdc_publication FOR TABLE feature_config;
-- 后续追加表: ALTER PUBLICATION cdc_publication ADD TABLE solution;

-- 3. (可选) UPDATE/DELETE 事件包含完整旧行
ALTER TABLE feature_config REPLICA IDENTITY FULL;

-- 4. 确认连接用户有 REPLICATION 权限
ALTER ROLE repl_user REPLICATION;
```

### 2. 应用配置

配置优先级：**CLI 参数 > 环境变量 > YAML 配置文件 > 默认值**

```bash
cp examples/config.example.yml config.yml
# 编辑填入实际值，敏感字段建议走环境变量
```

### 3. 构建与运行

```bash
# 开发
cargo run -- --config config.yml

# Docker
docker compose up -d
```

## 消息格式

```json
{
  "op": "insert | update | delete | sync",
  "table": "public.solution",
  "before": null,
  "after": { "id": 1, "solution_id": "abc" }
}
```

- `op=sync`: 初始全量同步阶段
- Message Key: 主键列值（保证同行事件进同一分区）

## 配置参考

完整参数见 `examples/config.example.yml` 和英文 README。支持 Postgres SSH 隧道 (密码/私钥)、Kafka 多种安全协议 (plaintext/ssl/sasl_plaintext/sasl_ssl)。

### Kafka 目标路由

所有路由通过 `kafka.targets` 配置 —— 一个扁平的对等列表。每个 target 独立定义 `topic_prefix` 和可选的 `tables` 过滤器，不存在"主从"或"继承"关系。

`kafka` 顶层字段（`brokers`、认证参数）是所有 target 的共享连接默认值；每个 target 可覆盖。当所有 target 都自带 `brokers` 时，顶层 `kafka.brokers` 可以不填。

| 字段 | 说明 | 必填 |
|------|------|------|
| `name` | 目标名称（唯一） | 是 |
| `topic_prefix` | Topic 前缀：`{prefix}.{schema}.{table}` | 是 |
| `tables` | 表白名单（`schema.table` 格式），不填则转发全部 | 否 |
| `brokers` | 覆盖共享 brokers | 否（若顶层未设置则必填） |
| `security_protocol` | 覆盖共享安全协议 | 否 |
| 其他认证字段 | 覆盖共享认证配置（sasl_*、ssl_*） | 否 |

事件会投递到 **所有 `tables` 过滤匹配的 target**。若某表不在任何 target 的列表中，对应事件静默丢弃。

**CLI 向后兼容**：YAML 中未定义 `kafka.targets` 时，`--kafka-topic-prefix` 自动创建单个默认 target。

### 多 topic 扇出（同一集群）

多个 target 共享同一 Kafka 集群，只需设一次 `kafka.brokers`，各 target 用不同的 `topic_prefix` / `tables`：

```yaml
kafka:
  brokers: 10.9.169.2:9092,10.9.131.68:9092
  security_protocol: plaintext

  targets:
    - name: primary
      topic_prefix: cdc
      tables:
        - public.table1

    - name: backup
      topic_prefix: cdc-backup       # 不设 tables → 收全部表

    - name: analytics
      topic_prefix: analytics
      tables:
        - public.table2
```

完整示例见 `examples/config.fanout.example.yml`。

### 多 Kafka 集群（每个 target 独立 brokers）

每个 target 可独立指定自己的 `brokers` 和认证，连接完全不同的 Kafka 集群。所有 target 自带连接信息时，顶层 `kafka.brokers` 可省略：

```yaml
kafka:
  # 不设共享 brokers —— 每个 target 完全自包含
  targets:
    - name: cluster-a
      brokers: kafka-a1:9092,kafka-a2:9092
      security_protocol: plaintext
      topic_prefix: cdc.cluster-a
      tables:
        - public.orders
        - public.payments

    - name: cluster-b
      brokers: kafka-b1:9092,kafka-b2:9092,kafka-b3:9092
      security_protocol: plaintext
      topic_prefix: cdc.cluster-b
      tables:
        - public.orders
        - public.user_events

    - name: cluster-c-prod
      brokers: kafka-prod.example.com:9094
      security_protocol: sasl_ssl
      sasl_mechanism: scram-sha-512
      sasl_username: pgpour_producer
      # sasl_password: (通过环境变量设置)
      ssl_ca_location: /etc/pgpour/ca-prod.pem
      topic_prefix: prod.analytics
      tables:
        - public.user_events
        - public.analytics_log
```

每个 target 创建独立的 Kafka producer —— 不同 brokers、不同认证、不同压缩上下文。完整示例见 `examples/config.multi-kafka.example.yml`。

## 性能

- **批量投递**：CDC 事件先全量序列化，再通过 `try_join_all` 并发投递到 Kafka，librdkafka 自动合并网络请求
- **lz4 压缩**：生产者级别压缩 JSON 负载，网络 I/O 减少 50–80%
- **典型吞吐**：~20,000 events/sec（~15 MB/s 负载），单核 CPU < 1%

## 可选功能

- `--features metric`: 启用 OpenTelemetry OTLP 指标导出

## License

MIT
