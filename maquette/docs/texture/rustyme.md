# Maquette × Rustyme (sonargrid) — worker contract

**Audience.** Anyone writing a worker that Maquette's
`RustymeProvider` will enqueue tasks for. Maquette is strictly a
**producer**: it pushes tasks onto a Redis list and waits for a
reply on another list. Workers are owned and operated separately
(see [`sonargrid`](../../public_work/sonargrid/)).

> **Implementing the worker?** Read
> [`rustyme-worker-roadmap.md`](./rustyme-worker-roadmap.md) next
> — that's the stage-by-stage plan (Echo → Fal.ai → prod
> hardening) with code skeletons and a joint acceptance
> checklist. The current file is the *wire protocol*; the
> roadmap is *how to ship*.

If you change this contract, bump the `task` name (e.g. from
`texture.gen` to `texture.gen.v2`) instead of silently mutating
the payload — Maquette caches aggressively by SHA-256 so a quiet
shape drift will produce wrong bytes on disk that only surface
months later.

## Wiring overview

```
Maquette GUI / CLI          sonargrid                          Worker
  (producer)                (queue)                            (consumer)

  LPUSH rustyme:texgen:queue <envelope>  ──▶   actor dispatcher  ──▶  picks up task
                                                                        │
                                                                        │ calls Fal / SDXL / …
                                                                        ▼
     BRPOP rustyme:texgen:result  ◀──  LPUSH result back with PNG bytes
```

Exact key names are configurable on both sides — Maquette reads
them from env vars (`MAQUETTE_RUSTYME_QUEUE_KEY` /
`MAQUETTE_RUSTYME_RESULT_KEY`), Rustyme reads them from
`QUEUE_N_KEY` / `QUEUE_N_RESULT_KEY`. Keep them in sync.

## Task envelope (Maquette → worker)

Maquette produces a [`TaskEnvelope`][env-rs] with:

| field            | value                                        |
|------------------|----------------------------------------------|
| `id`             | fresh UUID v4                                |
| `task`           | `texture.gen` (override: `MAQUETTE_RUSTYME_TASK_NAME`) |
| `args`           | empty                                        |
| `kwargs`         | see below                                    |
| `max_retries`    | `MAQUETTE_RUSTYME_MAX_RETRIES` (default `3`) |
| `priority`       | `"normal"`                                   |
| `unique_for_secs`| `3600`                                       |

`kwargs` shape — all fields required:

```json
{
    "prompt":    "isometric grass block, low-poly, seamless",
    "seed":      42,
    "width":     128,
    "height":    128,
    "model":     "fal-ai/flux/schnell",
    "cache_key": "a1b2c3d4…f4"
}
```

* `cache_key` is the SHA-256 of
  `prompt | seed | width | height | model` computed by Maquette.
  Workers are free to ignore it, but it's handy for
  worker-side dedup + log correlation.
* `model` is the *upstream* model id you should pass to your
  provider. Maquette doesn't enforce a particular value — if your
  worker only knows how to run FLUX schnell, either refuse tasks
  whose `model` you don't recognise (set `status=FAILURE`) or
  document a fallback.

[env-rs]: ../../public_work/sonargrid/rustyme-core/src/protocol.rs

## Result envelope (worker → Maquette)

Workers `LPUSH` the following JSON onto `result_key`:

```json
{
    "task_id": "<the envelope's id, echoed verbatim>",
    "status":  "SUCCESS",
    "result": {
        "png_b64": "<standard base64 (RFC 4648) of raw PNG bytes>"
    },
    "metadata": null
}
```

Failures:

```json
{
    "task_id": "<envelope id>",
    "status":  "FAILURE",
    "result":  null,
    "error":   "Fal returned 429: rate-limited"
}
```

Maquette matches by `task_id`. Foreign task ids (i.e. from
another concurrent producer) are `RPUSH`-ed back to the tail so
they aren't lost. Any response where `status != "SUCCESS"` is
surfaced to the caller as a provider error — **always populate
`error` with an actionable message**, don't silently succeed-with-null.

### Why base64 and not a URL

Inline base64 keeps the happy path one Redis round-trip. At
128 × 128 PNG you're talking ~50 KB (~67 KB base64), well under
Redis's 512 MB value cap. If you need to ship higher-resolution
results later, bump the `task` name to `texture.gen.v2` and
switch `result` to `{"url": "https://…", "sha256": "…"}`; Maquette
will then have to download + verify, which is an explicit
behavioural change, not a silent one.

## Timeouts, revocation, purge

* **Client-side timeout.** Maquette waits up to
  `MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS` (default 60) for a
  reply. On timeout it:
  1. stops BRPOP,
  2. issues `POST /api/admin/tasks/{id}/revoke` best-effort,
  3. returns `ProviderError::Remote("rustyme: timed out after …")`.

  Workers should honour Rustyme's `Revoked` state — a worker
  that picks up a revoked task after timeout has *wasted a GPU
  call* and will burn money. Rustyme's framework handles the
  "revoke before process starts" case for you; the remaining
  risk is "worker mid-flight when revoke lands" which is
  intrinsic and not worth fixing in Phase B.

* **Manual revoke.** `maquette-cli texture revoke <task_id>
  --admin-url http://…` issues the same Admin call. Useful when
  a user closes the progress modal mid-gen.

* **Queue purge.** `maquette-cli texture purge <queue>
  --admin-url http://…` wipes pending tasks from a named queue
  (not the raw Redis key — Rustyme's logical queue name). Only
  run during cluster recovery.

## Env-var reference

| Var                                      | Required | Default                     | What it does                                      |
|------------------------------------------|:-------:|-----------------------------|---------------------------------------------------|
| `MAQUETTE_RUSTYME_REDIS_URL`             |  yes    | —                           | `redis://host:port[/db]` to LPUSH/BRPOP against.  |
| `MAQUETTE_RUSTYME_QUEUE_KEY`             |         | `rustyme:texgen:queue`      | Must match worker's `QUEUE_N_KEY`.                |
| `MAQUETTE_RUSTYME_RESULT_KEY`            |         | `rustyme:texgen:result`     | Must match worker's `QUEUE_N_RESULT_KEY`.         |
| `MAQUETTE_RUSTYME_ADMIN_URL`             |         | —                           | Enables revoke/purge. Without it, timeouts don't cancel.|
| `MAQUETTE_RUSTYME_TASK_NAME`             |         | `texture.gen`               | Bump only on protocol breaks.                     |
| `MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS`   |         | `60`                        | Clamps Maquette's patience.                       |
| `MAQUETTE_RUSTYME_MAX_RETRIES`           |         | `3`                         | Propagated into envelope.                         |
| `MAQUETTE_RUSTYME_MODEL`                 |         | `rustyme:texture.gen`       | Written into the disk cache key. Change this when you swap worker fleets so stale PNGs don't resurface.|

## Smoke test

```bash
# 1. Bring up Rustyme locally (see sonargrid/README).
#    Register a worker for `texture.gen` that fulfills the
#    contract above.

# 2. Enqueue via Maquette.
export MAQUETTE_RUSTYME_REDIS_URL=redis://localhost:6379/0
export MAQUETTE_RUSTYME_ADMIN_URL=http://localhost:12121
cargo run --bin maquette-cli -- texture gen \
    --provider rustyme \
    --prompt "grass tile" \
    --seed 1 \
    --width 128 --height 128 \
    --no-cache \
    -o /tmp/grass.png

# 3. Second run with the same args hits the Maquette disk cache
#    (no Rustyme round-trip).
cargo run --bin maquette-cli -- texture gen \
    --provider rustyme \
    --prompt "grass tile" --seed 1 \
    -o /tmp/grass2.png

# 4. Operationally: clear a stuck queue.
cargo run --bin maquette-cli -- texture purge texgen
```

## Roadmap follow-ups (not in Phase B)

* Per-task `priority = "high"` when a GUI user clicks Generate
  (v0.10 D).
* `chord_callback` for batch generation: regenerate a whole
  palette in parallel and fire a single "all done" event into
  Maquette — worth doing once the GUI has a palette-wide
  Generate button.
* Object-store offload for > 512 KB results (only relevant if we
  ever ship 512² or 1024² textures).
