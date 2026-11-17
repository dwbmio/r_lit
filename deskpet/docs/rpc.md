# deskpet RPC API

Remote control surface for the deskpet desktop mascot. Lets external
tools send notifications, read pet state, and drive pet actions over
loopback TCP without spawning a UI.

## Quick start

```bash
# Start the mascot. Logs will show both servers' ports:
#   deskpet: NDJSON RPC listening on 127.0.0.1:47800
#   deskpet: HTTP RPC + Swagger UI on http://127.0.0.1:47801/
./deskpet

# From another terminal:
./deskpet call help/methods --json

./deskpet call notification/show \
    -p '{"body":"build failed","level":"error","title":"Build"}'

./deskpet call pet/state --json

# Browse the full API + try requests in the browser:
open http://127.0.0.1:47801/
```

## Surfaces

| Transport | Address | Purpose |
|-----------|---------|---------|
| NDJSON    | `127.0.0.1:47800` (`DESKPET_RPC_PORT`) | Long-lived connection, one request per line, one response per line. For CLI / scripts. |
| HTTP      | `http://127.0.0.1:47801/` (`DESKPET_HTTP_PORT`) | Swagger UI for browsing, `POST /m/<method>` for one-shot calls, `GET /openapi.json` for the machine-readable spec. For browser / IDE / third-party tools. |

Both surfaces dispatch through the same `MethodRegistry`, so behavior
is identical — the HTTP server just translates `(method, body)` ↔
NDJSON request/response shape.

## Wire format

JSON-RPC 2.0-inspired over NDJSON (one JSON object per line). The
`jsonrpc: "2.0"` field is omitted (we use NDJSON framing, not HTTP
body). `id` is always `u64`.

### NDJSON request (one line)

```json
{"id": 1, "method": "notification/show", "params": {"body": "hi", "level": "info"}}
```

### NDJSON response (one line)

Success:
```json
{"id": 1, "result": {"shown": true, "queued": false}}
```

Error:
```json
{"id": 1, "error": {"code": -32601, "message": "method not found: foo", "data": {"method": "foo"}}}
```

### HTTP request

`POST /m/notification/show` with `Content-Type: application/json`:

```json
{"id": 1, "params": {"body": "hi", "level": "info"}}
```

The `method` is taken from the URL path. If `method` is also in the
body, the URL path wins.

### HTTP response

Same JSON shape as NDJSON. Status codes:
- `200` — result returned
- `400` — parse error / invalid request / invalid params
- `409` — action not applicable in current state
- `422` — invalid action parameters
- `429` — notification dropped (queue full)
- `500` — internal error

## Methods

| Name | Description |
|------|-------------|
| `help/methods` | List every registered method + protocol version |
| `notification/show` | Show a reminder above the mascot |
| `notification/clear` | Dismiss current reminder + drop queue |
| `notification/list` | Return current + queued reminders |
| `pet/state` | Snapshot of pet position, walk params, mascot kind, window visibility |
| `pet/control` | Dispatch action: `hop`, `show`, `hide`, `set_speed` |

### `notification/show`

```json
// Request
{
  "id": 1,
  "method": "notification/show",
  "params": {
    "body": "build failed",
    "title": "Build",                  // optional
    "level": "error",                  // info | success | warn | error
    "duration_ms": 8000,               // optional, clamped to [800, 120000]
    "id": "build-7f3a"                 // optional, for dedup (see below)
  }
}

// Response
{
  "id": 1,
  "result": {
    "shown": true,                     // currently on screen
    "queued": false,                   // queued behind existing reminder
    "suppressed": false,               // OS toast suppressed due to dedup/throttle
    "id": "build-7f3a"                 // echoed from request, null if not provided
  }
}
```

**Dedup + throttle** (OS toast is the only display surface):
- When `id` is supplied, the OS toast is suppressed if the same id fired
  within `DESKPET_NOTIFY_DEDUP_WINDOW_MS` (default 60s).
- OS toasts are also rate-limited:
  - Min interval: `DESKPET_NOTIFY_MIN_INTERVAL_MS` (default 2000ms)
  - Per-minute cap: `DESKPET_NOTIFY_MAX_PER_MINUTE` (default 30)
- `suppressed=true` in the response means the OS toast was skipped — nothing
  is displayed for that call (the in-app egui bubble was removed; the mascot
  still pops up and hops). Use short retry loops with care.

### `notification/clear`

```json
// Request: no params
{"id": 2, "method": "notification/clear", "params": {}}

// Response
{"id": 2, "result": {"dropped": 3}}    // queue was 3 deep, now 0
```

### `notification/list`

```json
// Response
{
  "id": 3,
  "result": {
    "current": {
      "title": "Build",
      "body": "3 errors",
      "level": "error"
    },
    "queue": [
      {"title": null, "body": "queued 1", "level": "info"}
    ]
  }
}
```

### `pet/state`

```json
// Request: no params
{"id": 4, "method": "pet/state", "params": {}}

// Response
{
  "id": 4,
  "result": {
    "position": [150.0, 240.0],        // [x, y] physical pixels
    "walk_speed": 50.0,                // px/s, 0..=1000
    "walk_target": 320.0,
    "moving": true,
    "is_slime": true,                  // false = GLB mascot loaded
    "mascot_glb": "",                  // filename when !is_slime
    "visible": true
  }
}
```

### `pet/control`

```json
// Request
{
  "id": 5,
  "method": "pet/control",
  "params": {
    "action": "hop"                    // hop | show | hide | set_speed | walk_to | set_wander | quit
    // "value":  80.0                  // required for set_speed, [0.0, 1000.0]
    // "x":      200.0                 // required for walk_to
    // "y":      300.0                 // required for walk_to
    // "enabled": true                 // required for set_wander
  }
}

// Response
{"id": 5, "result": {"applied": true, "note": "hop_request=true"}}
```

Actions:

| Action | Params | Effect |
|--------|--------|--------|
| `hop` | none | Trigger vertical hop (sets `hop_request`, consumed by `consume_hop`). |
| `show` | none | Set `window.visible = true`. |
| `hide` | none | Set `window.visible = false`. |
| `set_speed` | `value: f32` in `[0.0, 1000.0]` | Update `walk_speed`. |
| `walk_to` | `x: f32, y: f32` in `[-100000.0, 100000.0]` | Instant teleport. Halts any in-progress walk so `apply_window_pos` doesn't fight the new position. |
| `set_wander` | `enabled: bool` | Toggle idle wander on/off. |
| `set_glb` | `name: String` (filename only, no `/` or `..`) | Switch to a GLB mascot (e.g. "block.glb", "blast.glb"). |
| `reset_position` | none | Re-center the pet window on the primary monitor. |
| `quit` | none | Send Bevy `AppExit`. Reply goes out first, then process exits cleanly. |

Error codes returned:
- `-32602 InvalidParams` — required param missing (e.g. `walk_to` without `x`).
- `-32003 InvalidAction` — value out of range (e.g. `set_speed` value=2000, `walk_to` x=1e8).
- `-32002 ActionNotApplicable` — current pet state forbids the action.

### `help/methods`

```json
// Response
{
  "id": 0,
  "result": {
    "protocol_version": 1,
    "build_version": "0.1.0",
    "methods": [
      {"name": "help/methods", "description": "...", "since": 1},
      {"name": "notification/clear", "description": "...", "since": 1},
      ...
    ]
  }
}
```

Call this first after connecting to discover the protocol version. The
version is bumped on backward-incompatible changes (signature changes,
method removal, error code reassignment). Adding new methods does NOT
bump the version.

## Error codes

JSON-RPC 2.0 standard codes:

| Code | Meaning |
|------|---------|
| `-32700` | Parse error — malformed JSON |
| `-32600` | Invalid request — JSON parsed but missing required fields |
| `-32601` | Method not found |
| `-32602` | Invalid params — type mismatch / missing field / out-of-range |
| `-32603` | Internal error — Bevy resource missing, etc. |

Application-specific (reserved server-error range):

| Code | Meaning |
|------|---------|
| `-32001` | `NotificationDropped` — reminder queue is full |
| `-32002` | `ActionNotApplicable` — current pet state forbids the action |
| `-32003` | `InvalidAction` — action parameters invalid (e.g. out-of-range) |

## Trust model

Both servers bind to `127.0.0.1` only. Any process running as the same
user can hit them. The full RPC power (including `pet/control` and
`notification/show`) is available with no auth.

This is acceptable for deskpet because:
- Loopback is the only attack surface — no inbound from the network.
- The deskpet process already runs with full user privileges (it owns
  the desktop, the tray icon, the camera cursor polling, etc.). No RPC
  gives a remote attacker more than they already had.

If you ever relax the bind address (`0.0.0.0`), add bearer-token auth
BEFORE doing so. The HTTP server has a `cors_header()` helper that
allows all origins — fine for loopback-only; tighten it before exposing.

## CLI

```bash
deskpet                             # Launch the mascot app
deskpet help                        # Print CLI help

deskpet send "build finished"       # Show a reminder (alias)
deskpet send -t Build -l error -m "3 errors"
deskpet send --clear

deskpet call <METHOD> -p '<json>'   # Generic RPC dispatch
deskpet call help/methods --json
deskpet call notification/show -p '{"body":"hi"}'
deskpet call pet/state --json
deskpet call pet/control -p '{"action":"hop"}'

deskpet call <METHOD> --json        # Pretty-print response
deskpet call <METHOD> -q            # Quiet (exit code only, no stdout)
```

Exit codes:
- `0` — `result` in response
- `1` — `error` in response, OR connection failed, OR read/write failed
- `2` — CLI usage error (missing args, bad JSON)

The `1 vs 0` distinction lets shell chains work: `deskpet call pet/state &&
deskpet call pet/control -p '{"action":"hop"}'`.

## Internals

- **`src/rpc/mod.rs`** — protocol types: `Request`, `Response`, `Outcome`,
  `ErrorData`, `parse_request`
- **`src/rpc/error.rs`** — `RpcError`, `ErrorCode`, `AppError`
- **`src/rpc/dispatch.rs`** — `Method` trait, `MethodRegistry`, `dispatch_request`
- **`src/rpc/bevy_bridge.rs`** — `RpcTaskInbox` + `drain_rpc_tasks` exclusive system
- **`src/rpc/server.rs`** — NDJSON TCP listener (one thread per connection)
- **`src/rpc/http.rs`** — HTTP server (tiny_http) + Swagger UI + OpenAPI
- **`src/rpc/openapi.rs`** — wraps `utoipa::OpenApi` for the spec
- **`src/rpc/swagger_html.rs`** — embedded Swagger UI HTML
- **`src/rpc/cli.rs`** — `send_cli` (alias) + `call_cli` (general)
- **`src/rpc_methods/`** — method implementations + OpenAPI annotations

Adding a new method:
1. Pick a method name (`ns/verb` style).
2. Add a params + result struct in the appropriate sub-file, with
   `#[derive(Deserialize, Serialize, utoipa::ToSchema)]`.
3. Implement `Method` for a unit struct.
4. Add a `#[utoipa::path]` annotated stub fn for the OpenAPI doc.
5. Register it in `rpc_methods::register_all()` + the `ApiDoc` derive's
   `paths(...)` + `components(schemas(...))`.