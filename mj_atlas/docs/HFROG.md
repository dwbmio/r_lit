# hfrog Mirror

Optional integration that pushes a copy of every saved project, exported atlas, and refreshed manifest to an [hfrog](https://github.com/dingcode-icu/hfrog) artifact registry. Enabled per-user via `~/.config/mj_atlas/config.toml`; disabled by default — local saves and packs work identically with or without hfrog.

## What gets uploaded

| Trigger | Artifacts |
|---|---|
| `mj_atlas pack` (CLI) | `<name>.png` + `<name>.json` (or `_<N>.png/json` for multi-bin) + `<name>.manifest.json` |
| GUI File → Save Project | `<project>.tpproj` |
| GUI File → Export As (viewer mode) | the exported file |

The runlog (`<name>.log`) is **not** mirrored — it's flushed after the mirror runs, so it's not yet on disk at upload time. It stays a local-only debug aid.

## Configuration

`~/.config/mj_atlas/config.toml` (Linux) / `~/Library/Application Support/mj_atlas/config.toml` (macOS) / `%APPDATA%\mj_atlas\config.toml` (Windows):

```toml
[hfrog]
enabled = true
endpoint = "https://hfrog.example.com"
token = ""                      # bearer token; leave empty for unauthenticated
default_runtime = "asset-pack"  # stamps every artifact for filtering in hfrog UI
s3_inc_id = 0                   # which S3 backend the server should write to
```

`enabled = false` (or empty `endpoint`) disables the mirror entirely. `Config::is_active()` requires both an enabled flag AND a non-blank endpoint.

## GUI controls

The Settings panel has a "hfrog Mirror" section at the bottom:

```
┌─ hfrog Mirror ──────────────────┐
│ ☐ Mirror to hfrog on Save / Export │
│ Endpoint: [https://...        ] │
│ Token:    [•••••              ] │
│           [Save settings] [Reset] │
│ ○ mirror disabled               │
└──────────────────────────────────┘
```

Edits are buffered until "Save settings" — the file isn't touched mid-typing. The status line at the bottom shows current effective state:

- `● mirror active` — enabled + endpoint set
- `○ enabled but endpoint missing` — config inconsistent
- `○ mirror disabled` — opt-in still off

## Naming convention on hfrog

Each artifact carries:

| Field | Value |
|---|---|
| `name` | `<project_name>.<file_kind>` — e.g. `myproj.atlas-png`, `myproj.tpproj`, `myproj.manifest` |
| `ver` | First 12 hex chars of SHA-256 over the atlas pixel buffer (CLI pack); `save-<unix_ts>` (GUI save); `export-<unix_ts>` (GUI export) |
| `runtime` | `default_runtime` from config, default `"asset-pack"` |
| `s3_key` | `mj_atlas/<project>/<ver>/<filename>` |
| `md5` | First 32 hex chars of SHA-256 of the file bytes (hfrog schema demands a 32-char string; we don't pull in an MD5 dep just for the column) |

Re-uploads of the same content are idempotent on the server — hfrog returns business code `1001 AlreadyExist`, which the client treats as success.

## Failure semantics

A failed upload **never aborts the local pipeline**:

- Network down → local pack still completes; runlog notes `hfrog: upload failed for X: <error>` per file
- Hfrog returns a 5xx → same as above
- Config malformed → `config: load failed, using defaults` warning; mirror silently skipped
- Endpoint set but unreachable → 30 s timeout, then per-file errors; runlog summary `mirror complete — 0 ok, N failed`

The local atlas + manifest + log are always written first; hfrog is mirror-best-effort. This means losing connectivity to hfrog leaves the user no worse off than before they enabled mirroring.

## Wire format (hfrog endpoint)

`PUT <endpoint>/artifactory/add_form_file` — `multipart/form-data` with two parts:

```
Content-Disposition: form-data; name="json"
Content-Type: text/plain

{"name":"...","ver":"...","md5":"...","cont_size":...,"runtime":"...",
 "s3_key":"...","s3_inc_id":0,"is_artifactory_ready":false,"is_raw":false,...}
```

```
Content-Disposition: form-data; name="file"; filename="atlas.png"
Content-Type: image/png

<bytes>
```

Auth: `Authorization: Bearer <token>` when `config.hfrog.token` is non-empty.

Server reply (regardless of HTTP path) is hfrog's `RespRet`:

```json
{ "code": 0, "msg": "" }
```

`code == 0` ⇒ success. `code == 1001` (`AlreadyExist`) ⇒ also treated as success.
Any other non-zero code surfaces in the runlog with the server's `msg`.

## CI use

Set the env-equivalent of the config (the file IS the source of truth — no env vars yet):

```bash
mkdir -p ~/.config/mj_atlas
cat > ~/.config/mj_atlas/config.toml <<EOF
[hfrog]
enabled = true
endpoint = "$HFROG_URL"
token = "$HFROG_TOKEN"
EOF

mj_atlas pack ./sprites -o atlas --pot --incremental --json
```

The pack returns its normal JSON summary on stdout; mirror successes / failures land in `<atlas>.log` next to the artifact.
