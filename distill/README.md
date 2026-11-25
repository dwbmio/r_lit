# Distill

Pure-Rust cross-functional knowledge distillation and P2P Q&A suite shipped as **two standalone binaries**. Distill collects and distills technical truth from internal Git repositories (across multiple branches concurrently) into sanitized semantic facts so product, QA, design, and operations teams can answer implementation questions using their **local AI** without ever receiving raw source code or SSH access.

- **`distilld`** (server daemon, e.g. on `ubuntu-hs-main`): Maintains bare Git mirrors and an LRU detached read-only worktree pool, extracts sanitized Semantic IR via Pure-Rust AST (`oxc_parser`) and Markdown/JSON parsers, caches per-`blob_oid` semantics with HKDF-SHA256 + ChaCha20-Poly1305 AEAD encryption, serves per-commit Tantivy indexes concurrently, enforces outbound BLAKE3 6-gram DLP anti-leak gating, and listens over `iroh` P2P QUIC (TLS 1.3 + Ed25519 `EndpointId` allowlist + rate limit + audit log).
- **`distill`** (interactive TUI, CLI & MCP bridge): Lightweight binary distributed to cross-functional teammates (zero Git fetch or source parsing logic). Connects to `distilld` over encrypted `iroh` QUIC (direct LAN or NAT traversal) and provides an interactive conversational **Ratatui TUI** (with a remote server status sidebar, mouse-clickable `Fetch` button, and branch switcher), a scriptable CLI, and a standard **MCP (Model Context Protocol) stdio server** for local AI assistants.

## Role-Specific Guides

- Agent Skill definition (`starlink-nebula` style): [SKILL.md](SKILL.md)
- Server deployment & operations (`distilld`): [docs/SERVER_DEPLOY_CN.md](docs/SERVER_DEPLOY_CN.md)
- Local client & MCP setup (`distill`, safe to share directly with end users): [docs/CLIENT_USAGE_CN.md](docs/CLIENT_USAGE_CN.md)
- CI/CD & MinIO release pipeline: [Jenkinsfile](Jenkinsfile) / [.justfile](.justfile)

## One-Line Install (MinIO)

```bash
bash -c "$(curl -fsSL https://synology-minio.youxi123.com/starlink-artifacts/distill/install.sh)"
```

## Extension Policy (No External Scripts)

- Zero runtime shell, Python, or Node scripts are used or permitted in deployment or extraction pipelines.
- When programmable per-repo extractors or custom DLP transforms become necessary, they must run strictly in-process via sandboxed **`mlua` (Luau)** with bounded memory/instruction budgets rather than spawning external script interpreters.

## Runtime Home Layout

- Server default root: `$HOME/.distill` (override with `DISTILL_HOME` or `--home`)
  - `server.key` / `master.key` (owner-only `0600`)
  - `config.toml` (whitelisted repos, allowed branch globs, authorized client `EndpointId` ACLs)
  - `repos/` (bare Git mirrors), `worktrees/` (LRU read-only worktree pool), `blob-cache/` (AEAD encrypted), `indexes/` (per-commit Tantivy), `audit.jsonl`
- Client default root: `$HOME/.distill-client` (override with `DISTILL_CLIENT_HOME` or `--home`)
  - `client.key` (`0600`), `client.toml`

## Build & Test

```bash
cd distill
cargo test --workspace
cargo build --release
```

Resulting binaries in `target/release/`:
- `distilld`
- `distill`

## Quick Start

```bash
# 1. Server side (initialize home and print server EndpointId)
distilld init --repo-url https://git.7k7k.com/starlink/twosite.git
distilld serve

# 2. Client side (generate client EndpointId, send to admin to add under [[clients]])
distill print-id
distill configure --server-id <SERVER_ENDPOINT_ID>
distill            # launches interactive conversational TUI (status sidebar + clickable Fetch & branch switch)
distill status
distill fetch --repo twosite
distill search --repo twosite --branch dev/v1 --query "qaValuePointRound1"
distill diff --repo twosite --base main --target dev/v1

# 3. Local AI MCP integration (stdio JSON-RPC 2.0)
distill mcp
```

Example MCP client registration:

```json
{
  "mcpServers": {
    "distill": {
      "command": "distill",
      "args": ["mcp"]
    }
  }
}
```
