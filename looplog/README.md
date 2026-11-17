# looplog

`looplog` is a local-first log intake and query tool for short AI-assisted debugging loops.

It stores logs in a local SQLite database, keeps only the recent debugging window by default, and exposes a loopback-only HTTP intake protocol so non-CLI tools can write logs from any language.

The first MVP is tuned for WeChat Mini Program debugging.

For the immediate WeChat Mini Program workflow that lets an AI agent inspect logs and repeat the debug loop through CLI queries, see [`WECHAT_AI_DEBUG_CN.md`](WECHAT_AI_DEBUG_CN.md).

## Install / Build

```bash
cargo build --release
```

## Quick Start

Start the local intake server:

```bash
looplog serve --addr 127.0.0.1:3768
```

Wrap a build command and attach WeChat metadata:

```bash
looplog run --tag miniprogram-build \
  --meta appid=wx123 \
  --meta project_path=./wxapp \
  --meta page=pages/index/index \
  -- npm run build
```

Query recent logs for AI consumption:

```bash
looplog list --kind wechat_miniprogram --appid wx123 --json
looplog grep TypeError --appid wx123 --page pages/index/index --since 2h --json
looplog show <run_id> --tail 200 --json
```

## Live side-by-side streams (real-device debugging)

When client and server logs live in different terminals, use `tail` to pull each
stream into the local database, then `view` to read them side by side in one window.

### 1. Attach each stream

`looplog tail` has five structured sources; the difference is whether the
timestamp and fields survive:

| `--from` | Command | Timestamp | Level | Extra fields |
|---|---|---|---|---|
| `journald` | `journalctl -o json -f` | `__REALTIME_TIMESTAMP` (µs) | `PRIORITY` 0-7 | pid / identifier / unit / hostname |
| `nginx` | `tail -F` + format parse | `time_local` from the line (with offset) | status 5xx→error / 4xx→warn | remote_addr / request / request_time / upstream_time |
| `logcat` | `adb logcat -v epoch -T N` | device epoch time | V/D→debug · I→info · W→warn · E/F/A→error | pid / tid / tag / priority |
| `file` | `tail -F` | receive time | keyword inference | — |
| `cmd` | any streaming command | receive time | keyword inference | — |

```bash
# server (journald; needs sudo for the system journal)
looplog tail --tag picboo-api --reconnect --sudo \
  --from journald --remote picboo-ali --unit picboo-api --since 15m

# edge (openresty access log; parses status and upstream timing)
looplog tail --tag picboo-edge --reconnect --sudo \
  --from nginx --remote picboo-ali \
  --path /opt/wirerope-stack/wirerope/logs/ding/c_picboo_access.log --lines 200

# client file (raw; timestamp is the receive time)
looplog tail --tag picboo-client --reconnect \
  --from file --path _ai/runs/weapp-dev-debug/latest.log --lines 0

# Android device (with several devices, pass the serial from `adb devices`)
looplog tail --tag android-logcat --reconnect \
  --from logcat --serial emulator-5554 --lines 200

# escape hatch: any command
looplog tail --tag anything --reconnect -- ssh host 'journalctl -o json -f'
```

Behaviour:

- **Receive time is stored separately** as `ingest_ms`; `ts_ms` keeps the source
  timestamp. The viewer appends `+820ms` / `+13.2h` markers when `|lag| ≥ 250ms`
  (negative values reveal clock skew).
- journald `PRIORITY` is often 6(info) for Rust tracing with the real level in the
  message, so severity is **only ever raised** from content; ANSI escapes are stripped.
- Several `tail` processes may share one database (WAL + busy_timeout + retry on
  concurrent `journal_mode` switching).
- `--reconnect` restarts with exponential backoff (1s→2s→…→`--reconnect-max`) into
  the same run.
  - **journald resumes precisely**: the highest stored `ts_ms` is replayed as
    `--since @<epoch>.<usec>` (one microsecond past it), so a dropped ssh session neither
    loses nor duplicates lines. Measured: second invocation used `--since @1792417000.000001`,
    2 rows, no duplicates.
  - `nginx` / `file` / `logcat` have no resumable cursor; a reconnect replays `--lines N`
    and can duplicate rows (known; semantics to be decided).
- Ctrl-C/Ctrl-\u200bTERM marks the run `interrupted` and kills the child (no orphans).
- Use `run` for one-shot backfill; `--since` (journald) / `--lines` (files/logcat) only prewarm.
- `logcat` always requests `epoch` so device-local timezones and the missing year in plain
  threadtime cannot skew the timeline. Unrecognised lines stay raw and use receive time.

### 2. One window for everything

```bash
looplog view --open \
  --column client:picboo-client \
  --column edge:picboo-edge \
  --column api:picboo-api \
  --column gql:picboo-gql
```

| Capability | Behaviour |
|---|---|
| Columns | one column per `label:tag`, independent scrolling |
| Merged | all tags on one timestamp-ordered timeline; best for `tap → edge 200 → api 503` causality |
| Filter + highlight | top bar input; `/regex/i` uses a regex, plain words highlight; matches also filter |
| Level | all / warn+ / error only / debug+ |
| Pause | stops polling; catching up on resume |
| Freeze | scrolling up stops appends and shows “↓ N new lines”; clicking it (or pressing `g`) returns **all columns** to live and resumes following |
| Copy | click any line to copy `time [tag] text` |
| Keys | `/` search, `p` pause, `m` columns/merged, `g` back to live |
| Discovery | without `--column`, the 4 most active recent tags become columns |
| **Timeline alignment** | "⏱ align" is on by default: scrolling one column binary-searches the others to the **same timeline position** (first row with `ts >= anchor`); the anchor is the top row of the column you scrolled |
| Time cursor | `⏱ HH:MM:SS.mmm` in the top bar is the current aligned position; each column header shows its latest timestamp (per-stream progress) |

`--open` uses Chrome `--app` mode on macOS (chromeless window); other platforms open
the default browser. `view` is **read-only**: no migration, no writes, safe next to
`tail` / `serve`.

Endpoints (reusable by a future native GUI):

```http
GET /                              # embedded single page
GET /api/lines?after_ms=&tags=a,b&limit=3000&tail=1
GET /api/tags?since_ms=
GET /healthz
```

### 3. Remote sudo (a tailed child has no tty)

`journalctl` needs privileges and cannot prompt for a password. Allow exactly the
needed commands:

```sudoers
# /etc/sudoers.d/looplog
picboo ALL=(root) NOPASSWD: /bin/journalctl -u picboo-api -f -o cat
picboo ALL=(root) NOPASSWD: /bin/journalctl -u picboo-gql -f -o cat
picboo ALL=(root) NOPASSWD: /usr/bin/tail -F /opt/wirerope-stack/wirerope/logs/ding/c_picboo_access.log
```

Or add the collector user to `systemd-journal`. ssh should be key-based via existing
ssh config / warpgate-generated hosts.

One command for the whole set (sample: `examples/picboo-dev.sh`):

```bash
./examples/picboo-dev.sh          # start 4 tails + open the viewer
```

## Rendering & performance

**The renderer is HTML/DOM** (`src/view/index.html`, embedded via `include_str!`, laid out by the
browser) — not egui, and not GPUI (GPUI is only a roadmap option for a native shell). The trade-off:
zero native deps and one codebase for macOS/Linux, but DOM has a cost ceiling, so it is pinned down:

| Mechanism | Implementation |
|---|---|
| Incremental render | steady state only appends via `insertAdjacentHTML('beforeend')`; full repaint only on filter/mode/clear |
| DOM ceiling | keep the last `1200` rows per column, batch-trim the head above `1500`, so node count stays flat |
| Index | `idx_lines_ts_seq(ts_ms, seq)` plus `INDEXED BY` to force the time range (otherwise the planner drives from `runs(tag)` and scans the whole window) |
| Cursor | `/api/lines?after_ms=` fetches only new rows; the client dedupes by `run#seq` |
| Tag aggregation | `/api/tags` aggregates `runs` (O(runs)) instead of scanning `log_lines` |
| Freeze | scrolling up stops appends and shows "↓ N new lines"; back at the bottom it catches up once |

Measured (1M-row DB, 16-minute window, 1000 lines/s sustained writes, 4 columns, Chrome/Playwright):

| Metric | Before | After |
|---|---|---|
| steady `/api/lines` (p50) | 74 ms | **0.8 ms** |
| first 3000 rows | 122 ms | **7.6 ms** |
| `/api/tags` | 312 ms | **0.7 ms** |
| fps / jank | 120fps / 0 long tasks | 120fps / 0 long tasks |
| DOM rows per column | grows with window | stable at 1200 |

`window.__looplog` exposes internal state in DevTools for debugging sync/render issues.

## Real-device log intake (domain channel)

Real devices push to a **sub-path of the business domain** — no extra DNS record, no new
certificate, no WeChat allowlist change:

```
mini program → https://dev-hs.picboo.ink/_looplog/loki/api/v1/push
             → picboo-ali nginx (POST only, 2m, 2s connect timeout)
             → 172.18.0.1:13768 (socat bridge)
             → 127.0.0.1:13768 (ssh -R, only while you run the on-demand tunnel)
             → your Mac's looplog serve:3768
```

| Env | Log endpoint | Note |
|---|---|---|
| loc | `http://127.0.0.1:3768` | in the simulator 127.0.0.1 *is* the Mac |
| dev | `https://dev-hs.picboo.ink/_looplog` | same domain as the API |
| prod | `https://prod-c.picboo.ink/_looplog` | same; don't ship this in a release build |

### No daemon on the Mac

The tunnel is **not installed as a launchd service**; start it on demand and Ctrl-C to stop:

```bash
just looplog-tunnel          # foreground tunnel (Ctrl-C to disconnect)
just looplog-tunnel status   # tunnel / remote listener / domain push probe
just looplog-tunnel off      # clear leftovers
```

⚠️ The tunnel target must be the Mac's **LAN IP**. Targeting `127.0.0.1` makes looplog treat
tunnelled traffic as local loopback and **skip the token check** — a real hole we hit during
testing; `just` now pins the LAN IP.

### Server side (canonical files in ci-all-in-one)

- `compose/ding/deployments/picboo/conf.d/{dev-hs,prod-c}-picboo.conf`: `location /_looplog/` → `172.18.0.1:13768`
- `compose/ding/deployments/picboo/host/looplog-tunnel-fwd.service`: socat bridges `172.18.0.1:13768` to host `127.0.0.1:13768`

Boundaries: POST only, body ≤ 2m, 401 without token, 502 within 2s when no tunnel (the client
backs off; business traffic is unaffected).

## Configuration

**Single source of truth**: `~/.config/looplog/config.toml` (`--config` overrides, honours `XDG_CONFIG_HOME`).

```bash
looplog config          # print path and contents
looplog config --init   # write a starter config (needs --force to overwrite)
```

```toml
version = 1

[view]
since = "15m"
autostart = true
columns = [
  { label = "client", tag = "picboo-client" },
  { label = "api",    tag = "picboo-api" },
]

[serve]
addr = "127.0.0.1:3768"
allow_lan = false
allow_cidrs = []

[[source]]
name = "picboo-api"
tag = "picboo-api"
from = "journald"       # journald | nginx | file | cmd | logcat
remote = "picboo-ali"
unit = "picboo-api"
since = "15m"
sudo = true
reconnect = true
enabled = true
```

### In-page management (⚙)

The top-bar gear opens a drawer that reads and writes that file:

| Capability | Behaviour |
|---|---|
| View | window, autostart, column list (one `label:tag` per line) |
| Source list | running/stopped dot, `from`, target (unit/path + ssh host), current run id |
| Start / stop | applies immediately, does not write the file |
| CRUD | add / edit / delete in a draft; only saving touches disk |
| Save & apply | `PUT /api/config` → write file (backs up to `config.toml.bak`) → restart affected sources |

Conventions:

- The file is authoritative; `view --column / --since` only set the initial values for one process.
- Start/stop changes runtime only; `enabled` drives autostart and what "save & apply" launches.
- Saving restarts every running source so the view never mixes old and new settings.
- The token is **never** written to the file; `serve --allow-lan --token` stays on the command line.

Sample: `examples/picboo-config.toml`; one-liner: `examples/picboo-dev.sh` (copies the sample on first run).

## HTTP Protocol

Loopback only by default. Real-device debugging needs an explicit opt-in:

```bash
# token must be >= 16 chars; non-loopback peers need it and must match --allow-cidr
# default allowlist: 127/8, ::1, 10/8, 172.16/12, 192.168/16, 169.254/16
looplog serve --addr 0.0.0.0:3768 --allow-lan --token "$(openssl rand -hex 16)"
```

Auth header: `Authorization: Bearer <token>` or `X-Looplog-Token: <token>`; loopback peers are exempt.

Two write paths:

| Endpoint | Purpose |
|---|---|
| `POST /v1/runs` → `POST /v1/runs/{id}/lines` | native (ndjson, used by `sdk/ts/looplog.ts`) |
| `POST /loki/api/v1/push` | **Loki compatible** (used by the mini program's existing `LokiClient`) |

### Loki compatible intake

The WeChat Mini Program already ships `LokiClient`; point its endpoint at looplog and
client logs flow in without a second reporter:

```json
{"streams":[{"stream":{"level":"info","source":"typescript","app_id":"picboo","looplog_tag":"picboo-client"},
  "values":[["1792465108984000000","{\"message\":\"login.start\",\"metadata\":{\"page\":\"pages/index/index\"}}"]]}]}
```

- nanosecond timestamps become `ts_ms` — the **client-side time is kept**, which is what
  makes lag/clock-skew visible
- `looplog_tag` → `app_id` → `source` decides `runs.tag` (used by `view --column`)
- JSON lines are decoded into `message` plus meta; plain lines are kept verbatim
- one run per label set per process, so a session does not fragment into many runs


```http
POST /v1/runs
POST /v1/runs/{run_id}/lines
PATCH /v1/runs/{run_id}
POST /loki/api/v1/push
GET /healthz
```

Create a run:

```json
{
  "tag": "wx-console",
  "source": "wechat-devtools",
  "kind": "wechat_miniprogram",
  "cwd": "/path/to/wxapp",
  "meta": {
    "project_path": "/path/to/wxapp",
    "appid": "wx123",
    "page": "pages/index/index",
    "compile_mode": "preview",
    "base_lib_version": "3.x",
    "platform": "devtools",
    "session": "ai-debug-001"
  }
}
```

Append log lines as `application/x-ndjson`:

```jsonl
{"stream":"console","level":"error","event":"console","text":"TypeError: Cannot read property x of undefined"}
{"stream":"network","level":"info","event":"request","text":"GET /api/user 200"}
```

Finish the run:

```json
{"status":"failed","exit_code":1}
```

## WeChat Metadata

Common fields are indexed for filtering:

- `kind`: usually `wechat_miniprogram`
- `project_path`
- `appid`
- `page`
- `session`
- `trace_id`

Additional fields are kept in `run_meta`, including `query`, `scene`, `compile_mode`, `tool_version`, `base_lib_version`, `platform`, `device`, and `network`.

## Retention

`looplog` is intentionally short-lived. The default retention window is 24 hours. `serve`, write commands, and query commands run lightweight cleanup automatically.

Manual cleanup:

```bash
looplog clean
looplog clean --keep-hours 6 --vacuum
```

Values above 24 hours are capped to 24 in the MVP.

## SDK

A small TypeScript client lives in `sdk/ts/looplog.ts`. A WeChat Mini Program adapter sample lives in `adapters/wechat/wechat_adapter.ts`.

The SDK calls `looplog serve`; it does not write SQLite directly.
