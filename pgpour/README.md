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
- Multi-topic fan-out: single replication slot, route events to different topics by table via `kafka_destinations`

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

<details>
<summary><b>Example: Direct PG connection (no tunnel)</b></summary>

```yaml
postgres:
  host: 10.0.1.50
  port: 5432
  database: mydb
  username: repl_user
  publication: cdc_publication
```

</details>

<details>
<summary><b>Example: SSH tunnel with password auth</b></summary>

```yaml
postgres:
  host: 10.0.1.50
  port: 5432
  database: mydb
  username: repl_user
  publication: cdc_publication
  ssh_host: bastion.example.com
  ssh_port: 22
  ssh_username: admin
  # ssh_password: (via PG_SSH_PASSWORD env var)
```

</details>

<details>
<summary><b>Example: SSH tunnel with private key</b></summary>

```yaml
postgres:
  host: 10.0.1.50
  port: 5432
  database: mydb
  username: repl_user
  publication: cdc_publication
  ssh_host: bastion.example.com
  ssh_username: deploy
  ssh_private_key_path: /etc/pgpour/id_ed25519
```

</details>

### Kafka Connection & Auth

| Env Var | YAML Field | Description | Default |
|---------|-----------|-------------|---------|
| `KAFKA_BROKERS` | `kafka.brokers` | Broker addresses | localhost:9092 |
| `KAFKA_TOPIC_PREFIX` | `kafka.topic_prefix` | Topic prefix | cdc |
| `KAFKA_SECURITY_PROTOCOL` | `kafka.security_protocol` | Security protocol | plaintext |

Security protocol determines additional required fields:

| Protocol | Description | Additional Fields |
|----------|-------------|-------------------|
| `plaintext` | No auth, no encryption (default) | None |
| `ssl` | TLS only | `ssl_ca_location` (mTLS: + `ssl_certificate_location` + `ssl_key_location`) |
| `sasl_plaintext` | SASL auth, plaintext | `sasl_mechanism` + `sasl_username` + `sasl_password` |
| `sasl_ssl` | SASL + TLS | SASL fields + SSL fields above |

<details>
<summary><b>Example: plaintext (dev/internal)</b></summary>

```yaml
kafka:
  brokers: 10.9.169.2:9092,10.9.131.68:9092
  topic_prefix: cdc
  security_protocol: plaintext
```

</details>

<details>
<summary><b>Example: sasl_ssl (production recommended)</b></summary>

```yaml
kafka:
  brokers: kafka.prod.example.com:9094
  topic_prefix: cdc
  security_protocol: sasl_ssl
  sasl_mechanism: scram-sha-512
  sasl_username: pgpour_producer
  # sasl_password: (via KAFKA_SASL_PASSWORD env var)
  ssl_ca_location: /etc/pgpour/ca.pem
```

</details>

### Multi-Topic Fan-out (Per-Target Table Filtering)

Add `kafka_destinations` in the YAML config to route CDC events to different topic prefixes within the same Kafka cluster. A single replication slot is used — WAL is decoded only once, and matching targets are produced to concurrently.

Each target supports an optional `tables` whitelist (`schema.table` format, e.g. `public.orders`); omit or leave empty to forward all tables. Events matching no target's `tables` list are silently dropped and the pipeline continues normally. Extra destinations inherit `brokers` and auth from the primary `kafka` section — only specify what differs (typically just `name`, `topic_prefix`, and `tables`).

```yaml
kafka:
  brokers: 10.9.169.2:9092,10.9.131.68:9092
  topic_prefix: cdc
  tables:                        # primary target only receives table1
    - public.table1

kafka_destinations:
  - name: backup
    topic_prefix: cdc-backup     # no 'tables' → receives all tables
  - name: analytics
    topic_prefix: analytics
    tables:                      # analytics only receives table2
      - public.table2
```

The primary `kafka` section (also configurable via CLI / env vars) is always the `"default"` target. `kafka_destinations` and `tables` are YAML-only. If you need to connect to a different Kafka cluster, override `brokers` / auth fields in that entry.

## Optional Features

- `--features metric`: OpenTelemetry OTLP metrics export

## License

MIT
