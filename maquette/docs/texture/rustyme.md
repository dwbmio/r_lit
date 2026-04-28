# Maquette × Rustyme (sonargrid) — worker contract

**Audience.** Anyone debugging the producer side of `texgen.gen`,
or writing/operating a worker that Maquette's `RustymeProvider`
will enqueue tasks for. Maquette is strictly a **producer**: it
pushes tasks onto a Redis list and waits for a reply on another
list. Workers are owned and operated separately — see
[`sonargrid`](../../public_work/sonargrid/) and the
business-facing reference
[`sonargrid/docs/texgen-queues.md`](../../public_work/sonargrid/docs/texgen-queues.md).

**Authoritative wire spec for workers.** sonargrid's
`docs/texgen-queues.md` is the *operator* facing reference and
defines what `kwargs` the workers will accept. Everything below
mirrors that document for the producer's perspective; when they
disagree, the sonargrid doc wins (and we file a Maquette patch).

> **Implementing a new worker variant?** Read
> [`rustyme-worker-roadmap.md`](./rustyme-worker-roadmap.md) next.
> The roadmap predates the actual sonargrid roll-out (which already
> shipped Stages 1-2 as the `texgen-cpu` + `texgen-fal` queues), so
> treat it as historical context for "how we got here". The current
> file is the *wire protocol* that's actually deployed.

If you change this contract, bump the `task` name (e.g. from
`texgen.gen` to `texgen.gen.v2`) instead of silently mutating
the payload — Maquette caches aggressively by SHA-256 so a quiet
shape drift will produce wrong bytes on disk that only surface
months later.

## Wiring overview

```
Maquette GUI / CLI                    sonargrid                              Worker (Lua hook)
  (producer)                           (queue)                               (consumer)

  LPUSH rustyme:texgen-cpu:queue <envelope>  ──▶   actor dispatcher  ──▶  texgen_cpu.lua  (free, ≤ 500 ms)
  LPUSH rustyme:texgen-fal:queue <envelope>  ──▶                     ──▶  texgen_fal.lua (Fal.ai, $0.003)
                                                                                │
                                                                                ▼
     BRPOP rustyme:texgen-{cpu,fal}:result  ◀──  LPUSH result back with image bytes
```

Two queues are deployed: `texgen-cpu` (programmatic CPU synthesis,
free + deterministic for `style_mode=solid`) and `texgen-fal`
(Fal.ai FLUX schnell, real AI imagery). They share the same task
name and envelope shape — only `kwargs` interpretation differs.

Exact key names are configurable on both sides — Maquette reads
them from env vars (`MAQUETTE_RUSTYME_QUEUE_KEY` /
`MAQUETTE_RUSTYME_RESULT_KEY`), Rustyme reads them from
`QUEUE_N_KEY` / `QUEUE_N_RESULT_KEY`. Keep them in sync.

## Picking a queue

Use the CPU lane unless you specifically need a real generative
model (concept art, UGC prompts, etc.). The decision tree from
sonargrid's operator doc applies verbatim:

| Need                                     | Queue        | `style_mode` | Cost       | Latency   |
|------------------------------------------|--------------|--------------|-----------:|----------:|
| Known palette / icon / placeholder       | `texgen-cpu` | `solid`      | 0          | ~30 ms    |
| Natural-language colour block / gradient | `texgen-cpu` | `auto`/`smart` | ~¥0.0002 | ~500 ms |
| Real AI imagery                          | `texgen-fal` | (n/a)        | ~$0.003    | 3-8 s     |

Pick via `MAQUETTE_RUSTYME_PROFILE=cpu` (default) or
`MAQUETTE_RUSTYME_PROFILE=fal`. The profile is purely a shorthand
for the `QUEUE_KEY` / `RESULT_KEY` pair — explicit env-var
overrides win.

## Task envelope (Maquette → worker)

Maquette produces a [`TaskEnvelope`][env-rs] with:

| field            | value                                                  |
|------------------|--------------------------------------------------------|
| `id`             | fresh UUID v4                                          |
| `task`           | `texgen.gen` (override: `MAQUETTE_RUSTYME_TASK_NAME`)  |
| `args`           | empty                                                  |
| `kwargs`         | see below                                              |
| `max_retries`    | `MAQUETTE_RUSTYME_MAX_RETRIES` (default `3`)           |
| `priority`       | `"normal"`                                             |
| `unique_for_secs`| `3600`                                                 |

`kwargs` shape:

```json
{
    "prompt":     "isometric grass block, low-poly, seamless",
    "seed":       42,
    "width":      128,
    "height":     128,
    "model":      "fal-ai/flux/schnell",
    "cache_key":  "a1b2c3d4…f4",
    "style_mode": "auto"
}
```

| field        | required | who reads it | description |
|--------------|:--------:|--------------|-------------|
| `prompt`     | ✅       | both         | Free-form prompt |
| `seed`       | ✅       | both         | RNG seed (deterministic on cpu/solid) |
| `width`      | ✅       | both         | Output width in pixels |
| `height`     | ✅       | both         | Output height in pixels |
| `model`      | ✅       | `texgen-fal` | Upstream model id |
| `cache_key`  | ✅       | both (logging) | SHA-256 of `prompt|seed|width|height|model` |
| `style_mode` | ❌       | `texgen-cpu` | `auto` (default) / `solid` / `smart` |

* `cache_key` is computed by Maquette and echoed verbatim. Workers
  use it for log correlation; it's not the de-dup key (Maquette
  already de-dups client-side via the disk cache).
* `model` is the *upstream* model id you should pass to your
  provider. Maquette doesn't enforce a particular value — if your
  worker only knows how to run FLUX schnell, either refuse tasks
  whose `model` you don't recognise (set `status=FAILURE`) or
  document a fallback.
* `style_mode` is sent only when `MAQUETTE_RUSTYME_STYLE_MODE` is
  set, otherwise the worker applies its own default (`auto` for
  CPU; ignored on Fal). **Caveat:** Maquette's disk cache is keyed
  by `TextureRequest`, which doesn't include `style_mode`. If you
  alternate between `solid` and `smart` with the same prompt+seed,
  the second run hits cached bytes from the first; pass
  `--no-cache` (or wipe `~/.cache/maquette/textures/`) when
  comparing modes side-by-side.

[env-rs]: ../../public_work/sonargrid/rustyme-core/src/protocol.rs

## Result envelope (worker → Maquette)

Workers `LPUSH` the following JSON onto `result_key`. **Two
shapes are accepted**, in order of preference:

### 1. New shape (`texgen-cpu` and `texgen-fal` use this today)

```json
{
    "task_id": "<the envelope's id, echoed verbatim>",
    "status":  "SUCCESS",
    "result": {
        "image_b64":    "<standard base64 (RFC 4648) of raw image bytes>",
        "format":       "png",
        "content_type": "image/png"
    },
    "metadata": null
}
```

The CPU worker also returns `style_mode`, `style_params`, and
optionally `llm` (provider, tokens, cost) inside `result` — all
ignored by Maquette but useful in logs.

**Maquette currently only consumes PNG bytes.** Any `format` other
than `"png"` is rejected with a clear error message; this is
deliberate since the disk cache filename is `<sha>.png` and the
v0.10 D GUI decoder assumes PNG. JPEG / WebP support arrives once
the decoder learns to branch on `content_type`.

### 2. Legacy shape (echo workers, pre-`texgen-cpu`)

```json
{
    "task_id": "...",
    "status":  "SUCCESS",
    "result": {
        "png_b64": "<standard base64 of PNG bytes>"
    }
}
```

Still accepted for backward compatibility; the `texgen-cpu` Lua
hook in fact emits both fields (`image_b64` *and* `png_b64`) when
`format == "png"`, and either path resolves to the same bytes on
the producer side.

### Failure

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
Redis's 512 MB value cap. If we need to ship higher-resolution
results later, bump the `task` name to `texgen.gen.v2` and switch
`result` to `{"url": "https://…", "sha256": "…"}`; Maquette will
then have to download + verify, which is an explicit behavioural
change, not a silent one.

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
  (the logical `texgen-cpu` / `texgen-fal`, not the raw Redis
  key). Only run during cluster recovery.

## Env-var reference

| Var                                      | Required | Default                          | What it does                                      |
|------------------------------------------|:-------:|----------------------------------|---------------------------------------------------|
| `MAQUETTE_RUSTYME_REDIS_URL`             |  yes    | —                                | `redis://host:port[/db]` to LPUSH/BRPOP against.  |
| `MAQUETTE_RUSTYME_PROFILE`               |         | `cpu`                            | Selects queue family (`cpu` / `fal`). Shorthand for the two `*_KEY` pairs. |
| `MAQUETTE_RUSTYME_QUEUE_KEY`             |         | `rustyme:texgen-cpu:queue`       | Must match worker's `QUEUE_N_KEY`. Overrides PROFILE. |
| `MAQUETTE_RUSTYME_RESULT_KEY`            |         | `rustyme:texgen-cpu:result`      | Must match worker's `QUEUE_N_RESULT_KEY`. Overrides PROFILE. |
| `MAQUETTE_RUSTYME_ADMIN_URL`             |         | —                                | Enables revoke/purge. Without it, timeouts don't cancel.|
| `MAQUETTE_RUSTYME_TASK_NAME`             |         | `texgen.gen`                     | Bump only on protocol breaks.                     |
| `MAQUETTE_RUSTYME_STYLE_MODE`            |         | (worker default = `auto`)        | Optional; `auto` / `solid` / `smart`. CPU-only.   |
| `MAQUETTE_RUSTYME_RESULT_TIMEOUT_SECS`   |         | `60`                             | Clamps Maquette's patience.                       |
| `MAQUETTE_RUSTYME_MAX_RETRIES`           |         | `3`                              | Propagated into envelope.                         |
| `MAQUETTE_RUSTYME_MODEL`                 |         | `rustyme:texture.gen`            | Written into the disk cache key **and** placed in `kwargs.model`. The default value is fine for `texgen-cpu` (the worker ignores `kwargs.model`). For **`texgen-fal` you must set this to a real Fal endpoint path** (e.g. `fal-ai/flux/schnell`) — sonargrid's fal Lua hook treats `kwargs.model` as the literal endpoint, so the default would resolve to `https://fal.run/rustyme:texture.gen` and 404. The `scripts/gen-mc-blocks.sh` helper sets it automatically on the fal lane. |

## Smoke test

Production sonargrid lives at `redis://10.100.85.15:6379` with
the Admin UI on `http://10.100.85.15:12121/ui`. To run a real
end-to-end check from a dev box that has network access:

```bash
export MAQUETTE_RUSTYME_REDIS_URL=redis://10.100.85.15:6379/0
export MAQUETTE_RUSTYME_ADMIN_URL=http://10.100.85.15:12121

# 1. CPU lane (free, ~30 ms for solid).
cargo run --bin maquette-cli -- texture gen \
    --provider rustyme \
    --prompt "grass tile" \
    --seed 1 \
    --width 128 --height 128 \
    --no-cache \
    -o /tmp/grass.png

file /tmp/grass.png      # → PNG image data, 128 x 128

# 2. Second run with the same args hits the Maquette disk cache
#    (no Rustyme round-trip).
cargo run --bin maquette-cli -- texture gen \
    --provider rustyme \
    --prompt "grass tile" --seed 1 \
    -o /tmp/grass2.png
# Expect: cmp /tmp/grass.png /tmp/grass2.png → identical

# 3. Switch to Fal lane (real AI; needs the worker to have FAL_KEY).
export MAQUETTE_RUSTYME_PROFILE=fal
cargo run --bin maquette-cli -- texture gen \
    --provider rustyme \
    --prompt "isometric grass block, low-poly, seamless" \
    --seed 42 --no-cache \
    -o /tmp/grass_fal.png

# 4. Operationally: clear a stuck queue.
cargo run --bin maquette-cli -- texture purge texgen-cpu \
    --admin-url $MAQUETTE_RUSTYME_ADMIN_URL
```

## Roadmap follow-ups (not in Phase B)

* Per-task `priority = "high"` when a GUI user clicks Generate
  (v0.10 D).
* `chord_callback` for batch generation: regenerate a whole
  palette in parallel and fire a single "all done" event into
  Maquette — worth doing once the GUI has a palette-wide
  Generate button.
* Multi-format consumption (JPEG / WebP). Worker side already
  emits these on demand; Maquette's GUI decoder + cache filename
  scheme need to learn `content_type` first.
* Object-store offload for > 512 KB results (only relevant if we
  ever ship 512² or 1024² textures).
