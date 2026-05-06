# Per-Worker Lua VM Matrix

Status: **prototype validated / not final gate close**  
Date: 2026-05-03  
Host: Tencent CVM `152.136.54.186`, local Redis, local sleep HTTP endpoint.

## What Changed

Rustyme prototype:

* `PhaseHook::clone_for_worker()` added as a minimal hook-cloning hook.
* `LuaPhaseHook::clone_for_worker()` loads one Lua VM per worker.
* `StrategyGroup` calls `clone_for_worker()` when spawning initial workers and
  scale-up workers.

This converts LuaJIT from:

```text
N workers → one shared Lua VM behind Mutex
```

to:

```text
N workers → N independent Lua VMs
```

## Long HTTP Matrix

Task: `GET http://127.0.0.1:18080/sleep?ms=2000`  
Metric collection includes worker `/proc/<pid>/stat` CPU deltas and RSS.

| Backend | Mode | Concurrency | Tasks | OK | Wall | Throughput | CPU ms/task | RSS after |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| Rustyme | LuaJIT per-worker VM | 4 | 100 | 100 | ~50.06s | ~2.0/s | not sampled in first 4x100 | not sampled |
| Celery | threads | 4 | 100 | 100 | ~50.13s | ~2.0/s | not sampled in first 4x100 | not sampled |
| Celery | prefork | 4 | 100 | 100 | ~50.13s | ~2.0/s | not sampled in first 4x100 | not sampled |
| Rustyme | LuaJIT per-worker VM | 16 | 400 | 400 | ~50.10s | ~7.98/s | **0.175** | **34.8 MB** |
| Celery | threads | 16 | 400 | 400 | ~50.17s | ~7.97/s | 1.35 | 47.4 MB |
| Celery | prefork | 16 | 400 | 400 | ~50.15s | ~7.98/s | 1.75 | 691.1 MB |
| Rustyme | LuaJIT per-worker VM | 32 | 800 | 800 | ~51.13s | ~15.65/s | **0.175** | **42.7 MB** |
| Celery | threads | 32 | 800 | 800 | ~51.25s | ~15.61/s | 1.29 | 49.3 MB |
| Celery | prefork | 32 | 800 | 800 | ~50.17s | ~15.94/s | 1.81 | 1.33 GB |

## Read

For fixed-concurrency long HTTP fan-out:

* Throughput is essentially bounded by `tasks / concurrency × 2s` for all
  correct implementations.
* After per-worker Lua VM, Rustyme matches Celery's correctness and expected
  timing at concurrency 4 / 16 / 32.
* Rustyme's main advantage is **efficiency**, not wall-clock at the same hard
  concurrency:
  * vs Celery threads: similar wall time, lower CPU ms/task (~7x lower at 16/32),
    slightly lower RSS.
  * vs Celery prefork: similar wall time, ~10x lower CPU ms/task and vastly lower
    RSS.
* Celery prefork is acceptable on correctness/perf but expensive in memory.
* Celery threads is memory-close but still higher CPU-time/task.

## Caveats

* This is a local HTTP sleep endpoint, not real Fal.ai internet.
* CPU accounting uses `/proc/<pid>/stat` tick deltas. It is good enough as an
  energy proxy, but not a wattmeter.
* Long-request result collection for Celery was fixed to out-of-order polling;
  older long-request rows without the fair collector should not be used for
  final comparison.
* Group/chord and failure recovery are still required before final go/no-go.

## Artifacts

Key summaries:

* `rustyme-lua-perworker-long-4x100.json`
* `rustyme-lua-perworker-long16-400.json`
* `rustyme-lua-perworker-long32-800.json`
* `celery-threads16-long-400.json`
* `celery-prefork16-long-400.json`
* `celery-threads32-long-800.json`
* `celery-prefork32-long-800.json`

Raw JSONL peers live under `../raw/`.

## Interim Judgment

Per-worker Lua VM gives Rustyme a credible advantage on the user's most important
long-request fan-out scenario:

* reliability restored,
* throughput matches Celery at the same fixed concurrency,
* CPU-time/task is materially lower,
* RSS is far below Celery prefork and slightly below Celery threads.

This is not the final gate pass because payload, group/chord, and failure
recovery remain, but this matrix supports continuing the Rustyme evaluation
rather than stopping immediately.
