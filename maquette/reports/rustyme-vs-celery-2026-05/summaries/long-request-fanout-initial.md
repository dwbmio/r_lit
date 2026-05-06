# Long-Request Fan-Out Initial Results

Status: **critical reliability finding / not final gate close**  
Date: 2026-05-03  
Host: Tencent CVM `152.136.54.186`, local Redis, local sleep HTTP server.

## Scenario

Workers call:

```text
GET http://127.0.0.1:18080/sleep?ms=2000
```

The sleep server returns `request_elapsed_ms` so the harness computes:

```text
non_request_ms = (producer_received - producer_sent) - request_elapsed_ms
```

The point is to compare queue/framework overhead under the common Maquette
fan-out shape: many slots each issue a long external request.

## Controls

Held constant:

* same machine
* same Redis
* same local HTTP sleep endpoint
* request duration ~2002 ms
* target worker concurrency = 4
* 100 fan-out tasks for primary run

## Results

| Backend | Worker mode | Tasks | OK | Timeout | Wall | Throughput | p95 end-to-end | p95 non-request | Notes |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| Celery | threads, concurrency=4 | 100 | 100 | 0 | ~50.13s | ~1.99/s | 48.08s | 46.07s | expected queue_wait staircase: 100 tasks / 4 workers / 2s |
| Celery | prefork, concurrency=4 | 100 | 100 | 0 | ~50.13s | ~1.99/s | 48.07s | 46.06s | essentially same as threads for local HTTP sleep |
| Celery | threads, concurrency=100 | 100 | 100 | 0 | ~4.85s | ~20.6/s | 3.94s | 1.94s | out-of-order collector; client HTTP gets noisy at 100 sockets |
| Rustyme | LuaJIT shared VM, min=max=4, timeout=180s | 100 | **1** | **99** | 180.8s | ~0.006/s | n/a | n/a | failed reliability gate |
| Rustyme | LuaJIT shared VM, min=max=4, timeout=180s | 4 | 4 | 0 | ~8.01s | ~0.50/s | 8.01s | 6.01s | tasks completed serially, not concurrently |
| Rustyme | LuaJIT **isolated per call**, min=max=4 | 4 | 4 | 0 | ~2.00s | ~2.0/s | 2.00s | 3.25ms | experimental env `RUSTYME_LUA_ISOLATED_PER_CALL=1` |
| Rustyme | LuaJIT **isolated per call**, min=max=4 | 100 | 100 | 0 | ~3.27s | ~30.5/s | 3.07s | 1.06s | restores concurrency; actual in-flight exceeds semantic worker=4 |
| Rustyme | LuaJIT **per-worker VM prototype**, min=max=4 | 4 | 4 | 0 | ~2.00s | ~2.0/s | 2.00s | 2.16ms | production-shaped ownership model |
| Rustyme | LuaJIT **per-worker VM prototype**, min=max=4 | 100 | 100 | 0 | ~50.06s | ~2.0/s | 50.05s | 48.05s | matches Celery concurrency=4 staircase |

## Important Findings

1. **Celery behaves as expected** for long-request fan-out. With 4 workers and
   100 × 2s requests, total time is ~50s. `non_request_ms` is dominated by
   queue_wait for later tasks, which is expected under fixed concurrency.

2. **Rustyme LuaJIT shared-VM long-request path failed the primary 100-task
   run.** Even with `min_workers=4`, `max_workers=4`, and `task_timeout=180`,
   only 1/100 returned before the producer timeout. Rustyme logs show many
   tasks dropped with:

   ```text
   [hook error]: task timeout after 180s
   ```

3. **The 4-task isolation run points to serialization/blocking inside the
   shared `LuaPhaseHook`**, not normal queue backlog. Four tasks with four
   workers should complete in ~2s. Instead, they completed over ~8s:

   ```text
   Rustyme LuaJIT 4 tasks:
   p50 end-to-end ≈ 8009 ms
   p95 non-request ≈ 6009 ms
   ```

   That looks like one long HTTP request executing at a time through the Lua
   hook path, despite a 4-worker strategy group.

4. The earlier autoscaler suspicion was real for the first failed run (it scaled
   4 → 2 → 1), but not sufficient to explain the fixed min/max run. After
   locking min/max at 4 and raising timeout, Rustyme still returned 1/100.

5. **Experimental per-call Lua VM isolation restores long-request concurrency.**
   With `RUSTYME_LUA_ISOLATED_PER_CALL=1`, four 2s HTTP tasks complete in ~2s,
   and 100 tasks complete in ~3.27s. This validates the shared Lua VM mutex as
   the proximate root cause. It also exposes a second semantic issue: Rustyme's
   actor `concurrency=4` is not a hard in-flight cap when handlers return
   asynchronous futures; once the Lua mutex is removed, effective in-flight can
   exceed four.

6. **Per-worker Lua VM prototype is the right production direction.** After
   adding `PhaseHook::clone_for_worker()` and letting `LuaPhaseHook` load one
   VM per worker, the same 4-task long HTTP run completes in ~2s and the
   100-task run completes in ~50s. That is the expected fixed-concurrency
   staircase (100 tasks / 4 workers × 2s). In other words:

   * shared VM = incorrect serialization,
   * per-call VM = diagnostic, restores concurrency but breaks worker-count
     semantics by permitting many in-flight futures,
   * per-worker VM = restores concurrency and preserves the Celery-like
     `concurrency=4` meaning.

## Root Cause Found in Code

`rustyme-lua/src/lib.rs` explains the observed behavior:

```rust
pub struct LuaPhaseHook {
    lua: Mutex<Lua>,
    // ...
}

async fn call_fn(&self, fn_name: &str, args: Vec<Value>) -> Result<Value, SonarError> {
    let lua = self.lua.lock().await;
    // ...
    let lua_result: LuaValue = func.call_async(...).await?;
    // lock is still held here
}
```

`rustyme/src/actor/group.rs` constructs all workers with the same shared
`Arc<dyn PhaseHook>`:

```rust
let workers: Vec<_> = (0..concurrency)
    .map(|i| WorkerActor::new(i, hook.clone(), redis_ctx.clone()).start())
    .collect();
```

Therefore a queue with `concurrency = 4` has four worker actors, but all of them
contend for the same Lua VM mutex. Because `call_fn()` holds the mutex across
`func.call_async(...).await`, long async builtins such as `http.get` serialize
the entire Lua hook path. This is not merely a logging artifact:

* 4 tasks × 2s HTTP → ~8s total.
* 100 tasks × 2s HTTP → ~200s theoretical total; with `task_timeout=180s`,
  most tasks time out.

This also explains the local-file IO result: Lua local file IO was reliable, but
it was effectively one Lua task at a time. It still finished quickly because
each task's file IO was only a few milliseconds.

## Candidate Fix

Rustyme needs one of these:

1. **Per-worker Lua VM** (best production shape, prototyped successfully): each
   `WorkerActor` gets its own `LuaPhaseHook` instance, so worker concurrency
   maps to Lua VM concurrency.
2. **Per-call Lua VM** (quick validation): reload the Lua script into a fresh VM
   for each phase call. This is slower for tiny no-op tasks but should restore
   long-request concurrency and is enough to validate the hypothesis.
3. **Release lock before await** is not practical with a single `mlua::Lua`
   state because the Lua coroutine and async builtin execution are tied to that
   state.

The prototype used `PhaseHook::clone_for_worker()` as the minimum viable
interface. A cleaner final patch may rename that concept to `HookFactory`, but
the ownership model is validated.

## Interpretation

This is a **red flag for Rustyme's default shared-VM LuaJIT hook model** under
long HTTP fan-out, which is exactly the workload Maquette cares about for
Fal-like texture generation. The per-worker VM prototype fixes the correctness
issue in the tested matrix, so Rustyme is not disqualified by this finding if
that ownership model becomes the default implementation.

It does not invalidate Rustyme's no-op EchoHook performance; that path remains
fast. But it means the gate cannot pass Rustyme until one of these is proven:

* Rustyme changes Lua hook ownership to per-worker Lua VM (production shape), or
* Maquette avoids Lua for long external requests and uses a Rust hook/provider
  path instead, which must then be benchmarked separately.

## Artifacts

Raw:

* `../raw/celery-threads-long-4x100.jsonl`
* `../raw/celery-prefork-long-4x100.jsonl`
* `../raw/rustyme-lua-long-fixed4-4x100.jsonl`
* `../raw/rustyme-lua-long-fixed4-4tasks.jsonl`
* `../raw/rustyme-lua-isolated-long-r2-4tasks.jsonl`
* `../raw/rustyme-lua-isolated-long-r2-4x100.jsonl`
* `../raw/rustyme-lua-perworker-long-4tasks.jsonl`
* `../raw/rustyme-lua-perworker-long-4x100.jsonl`

Summaries:

* `celery-threads-long-4x100.json`
* `celery-prefork-long-4x100.json`
* `rustyme-lua-long-fixed4-4x100.json`
* `rustyme-lua-long-fixed4-4tasks.json`
* `rustyme-lua-isolated-long-r2-4tasks.json`
* `rustyme-lua-isolated-long-r2-4x100.json`
* `rustyme-lua-perworker-long-4tasks.json`
* `rustyme-lua-perworker-long-4x100.json`

Logs:

* `../logs/rustyme-long-worker-fixed4.log`
* `../logs/rustyme-long-worker-fixed4-small.log`
* `../logs/celery-threads-long-worker.log`
* `../logs/celery-prefork-long-worker.log`

## Next Diagnostic

Before declaring Rustyme go/no-go, run the larger validation set:

1. Turn the per-worker Lua VM prototype into a clean Rustyme patch with tests:
   `PhaseHook::clone_for_worker()` or a more explicit `HookFactory`.
2. Rerun 400-task and 800-task long HTTP matrices to stress p95/p99.
3. Add worker CPU-time accounting and RSS snapshots for per-worker Lua vs Celery.
4. Run a Rust-native hook that performs the same local HTTP request.
   * If Rust-native is parallel and per-worker Lua is parallel: default shared
     Lua VM is the only blocker.
   * If Rust-native is also effectively unbounded, document the dispatch model
     and decide whether Maquette wants that behavior.
