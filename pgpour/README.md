# pgpour

Postgres CDC → Kafka real-time data pipeline. Built on [supabase/etl](https://github.com/supabase/etl), runs independently as a standalone ops-side service.

## Architecture

```
┌────────────┐  logical replication  ┌──────────┐  produce  ┌───────────────┐
│ PostgreSQL │ ────────────────────→ │ pgpour   │ ────────→ │ Topic-A (tbl1)│
│ (WAL)      │                       │          │ ────────→ │ Topic-B (tbl2)│
└────────────┘                       └──────────┘           └───────────────┘
```

- Zero intrusion on PG side (only requires `wal_level=logical` + `CREATE PUBLICATION`)
- One Kafka topic per table: `{prefix}.{schema}.{table}`
- Message format: JSON envelope (`op` / `before` / `after`)
- Multi-topic fan-out: single replication slot, independently route events to different topics via `kafka.targets`
- Batch producing with `lz4` compression for high throughput and low CPU overhead

## Prerequisites

- Rust >= 1.90.0, cmake
- PostgreSQL 14+ with `wal_level = logical`
- Connection user needs `REPLICATION` privilege

## Quick Start

### 1. Configure PostgreSQL

```sql
-- 1. Confirm wal_level (requires superuser; restart PG after changing)
SHOW wal_level;  -- must output: logical
-- ALTER SYSTEM SET wal_level = logical;  -- then restart PG

-- 2. Create publication for target tables
CREATE PUBLICATION cdc_publication FOR TABLE feature_config;
-- Add more tables later:
-- ALTER PUBLICATION cdc_publication ADD TABLE solution, feature_config_history;

-- 3. (Optional) Full old row in UPDATE/DELETE events
ALTER TABLE feature_config REPLICA IDENTITY FULL;

-- 4. Ensure connection user has REPLICATION privilege
ALTER ROLE repl_user REPLICATION;
```

### 2. Configure the Application

Config priority: **CLI args > env vars > YAML config file > defaults**

```bash
cp examples/config.example.yml config.yml
# Edit config.yml with actual values; use env vars for sensitive fields
```

### 3. Build & Run

```bash
# Development
cargo run -- --config config.yml

# Docker
docker compose up -d

# Or run the image directly with env var overrides
docker run -e PG_PASSWORD=xxx pgpour:latest
```

## Message Format

```json
{
  "op": "insert | update | delete | sync",
  "table": "public.solution",
  "before": null,
  "after": { "id": 1, "solution_id": "abc" }
}
```

- `op=sync`: Initial full sync phase row data
- Message Key: primary key column values (ensures same-row events go to same partition)

## Configuration Reference

All parameters support YAML config file / env vars / CLI args. See `examples/config.example.yml` for a complete example.

| Env Var | YAML Field | Description | Default |
|---------|-----------|-------------|---------|
| `CONFIG_PATH` | — | Path to YAML config file | — |
| `RUST_LOG` | — | Log level | info |

### Postgres Connection

| Env Var | YAML Field | Description | Default |
|---------|-----------|-------------|---------|
| `PG_HOST` | `postgres.host` | Host | (required) |
| `PG_PORT` | `postgres.port` | Port | 5432 |
| `PG_DATABASE` | `postgres.database` | Database name | (required) |
| `PG_USERNAME` | `postgres.username` | Username (needs REPLICATION) | (required) |
| `PG_PASSWORD` | `postgres.password` | Password | — |
| `PG_PUBLICATION` | `postgres.publication` | Publication name | cdc_publication |

### Postgres SSH Tunnel (Optional)

Without `ssh_host`, connects to PG directly. With it, automatically tunnels through the bastion host. Two auth methods:

| Auth Method | Required Fields |
|-------------|----------------|
| **Password** | `ssh_host` + `ssh_username` + `ssh_password` |
| **Private key** | `ssh_host` + `ssh_username` + `ssh_private_key_path` (add `ssh_private_key_passphrase` if key is encrypted) |

| Env Var | YAML Field | Description | Default |
|---------|-----------|-------------|---------|
| `PG_SSH_HOST` | `postgres.ssh_host` | Bastion host (enables tunnel) | — |
| `PG_SSH_PORT` | `postgres.ssh_port` | SSH port | 22 |
| `PG_SSH_USERNAME` | `postgres.ssh_username` | SSH username | (required when tunnel enabled) |
| `PG_SSH_PASSWORD` | `postgres.ssh_password` | SSH password | — |
| `PG_SSH_PRIVATE_KEY_PATH` | `postgres.ssh_private_key_path` | SSH private key path | — |
| `PG_SSH_PRIVATE_KEY_PASSPHRASE` | `postgres.ssh_private_key_passphrase` | Key passphrase | — |

### Kafka Connection & Auth

The `kafka` section in YAML defines shared connection defaults for all targets. Each target inherits these unless overridden per-target.

| Env Var | YAML Field | Description | Default |
|---------|-----------|-------------|---------|
| `KAFKA_BROKERS` | `kafka.brokers` | Broker addresses | localhost:9092 |
| `KAFKA_SECURITY_PROTOCOL` | `kafka.security_protocol` | Security protocol | plaintext |

Security protocol determines additional required fields:

| Protocol | Description | Additional Fields |
|----------|-------------|-------------------|
| `plaintext` | No auth, no encryption (default) | None |
| `ssl` | TLS only | `ssl_ca_location` (mTLS: + `ssl_certificate_location` + `ssl_key_location`) |
| `sasl_plaintext` | SASL auth, plaintext | `sasl_mechanism` + `sasl_username` + `sasl_password` |
| `sasl_ssl` | SASL + TLS | SASL fields + SSL fields above |

### Kafka Targets (Routing)

All routing is configured via `kafka.targets` — a flat list of independent peers. Each target defines its own `topic_prefix` and optional `tables` filter. There is no "primary" or parent-child relationship between targets.

| Field | Description | Required |
|-------|-------------|----------|
| `name` | Human-readable target name (must be unique) | Yes |
| `topic_prefix` | Topic prefix: `{prefix}.{schema}.{table}` | Yes |
| `tables` | Table whitelist (`schema.table` format). Omit to forward all. | No |
| `brokers` | Override shared brokers for this target | No (required if shared `kafka.brokers` is not set) |
| `security_protocol` | Override shared security protocol | No |
| Other auth fields | Override shared auth (sasl_*, ssl_*) for this target | No |

Events are produced to **all targets whose `tables` filter matches**. If a table is not in any target's list, its events are silently dropped.

**CLI backward compatibility**: when no `kafka.targets` are defined in YAML, `--kafka-topic-prefix` creates a single default target automatically.

### Multi-Topic Fan-out (Same Cluster)

When multiple targets share the same Kafka cluster, set `kafka.brokers` once and define targets with different `topic_prefix` / `tables`:

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
      topic_prefix: cdc-backup       # no 'tables' → receives all tables

    - name: analytics
      topic_prefix: analytics
      tables:
        - public.table2
```

See `examples/config.fanout.example.yml` for a complete example.

### Multi-Kafka Cluster (Different Brokers per Target)

Each target can independently specify its own `brokers` and authentication, connecting to a completely different Kafka cluster. When all targets are self-contained, `kafka.brokers` at the top level can be omitted entirely:

```yaml
kafka:
  # No shared brokers — each target is fully self-contained.
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
      # sasl_password: (use env or vault)
      ssl_ca_location: /etc/pgpour/ca-prod.pem
      topic_prefix: prod.analytics
      tables:
        - public.user_events
        - public.analytics_log
```

Each target creates an independent Kafka producer — different brokers, different auth, different compression contexts. See `examples/config.multi-kafka.example.yml` for a full working example.

## Performance

- **Batch producing**: CDC events are serialized in bulk, then produced to Kafka concurrently via `try_join_all`. This lets librdkafka batch network requests automatically.
- **lz4 compression**: JSON payloads are compressed at the producer level, reducing network I/O by 50–80%.
- **Typical throughput**: ~20,000 events/sec (~15 MB/s payload) with sub-1% CPU on a single core.

## Optional Features

- `--features metric`: OpenTelemetry OTLP metrics export

## License

MIT
