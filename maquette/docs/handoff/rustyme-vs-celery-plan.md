# Rustyme vs Celery 复测关口计划

Status: **active gate / initial no-op data collected**  
Owner: Maquette / Rustyme evaluation gate  
Decision scope: whether Maquette should continue investing in Rustyme as its
task-queue backend, switch to Celery, or add a `TaskQueueProvider` compatibility
layer.

## Non-Negotiable Decision Guard

No roadmap decision may be made from a single happy-path benchmark.

The current initial result (`phase0-2-initial.md`) is intentionally marked as
**no-op EchoHook only**. It is useful for runtime overhead, but it is not enough
to decide the backend for Maquette. The gate is only complete when all required
classes below have raw artifacts and summaries:

1. Rustyme built-in EchoHook no-op
2. Rustyme Lua hook no-op
3. Celery prefork no-op
4. Celery threads no-op (or a documented reason it is not applicable)
5. 64 KiB and 256 KiB payload runs on both systems
6. group/chord correctness on both systems
7. stale result / worker kill / Redis restart / timeout-retry-DLQ / revoke
8. Maquette workload replay: one-slot, 12-slot Generate all, Fal-like sleep
9. CPU-time-per-task and RSS-per-task for energy proxy
10. **long-request fan-out overhead**: Rustyme + LuaJIT vs Celery workers
    issuing many concurrent long requests, with external-request duration
    subtracted out so we measure queue/framework overhead

If any required class is missing, the report must say **"no final decision"**.

## Why This Gate Exists

Maquette now depends on async texture generation for its core loop:

```text
Material drawer / palette slot
  → task queue
  → CPU or Fal worker
  → PNG result
  → disk cache
  → Textured preview
```

If Rustyme is materially weaker than Celery on equivalent features, continuing
to build Maquette, hfrog, and texture-generation workflows around it is the
wrong technical bet. This gate makes that decision with measurements instead of
preference.

## Titan-Forge Machine Archaeology

The user asked whether the previous dynamic Tencent Cloud machine used by
`/Users/admin/titan-forge` could be reused.

Findings from local docs / history:

| Item | Finding |
|------|---------|
| Historical Tencent machine | `腾讯云 SA9e.2XLARGE16`, AMD EPYC 9K85 Turin, **8 vCPU / 16 GB**, Ubuntu 22.04, kernel 5.15 |
| Date | 2026-04-20 |
| Archived results | `/Users/admin/titan-forge/benches/linux-20260420/` and sibling `linux-fps-*` folders |
| Public IP | **Not preserved** in docs / transcripts / `~/.ssh/known_hosts` |
| Likely lifecycle | On-demand CVM created for bench, then released |
| Reusable value | machine spec, OS tuning checklist, cost model, deployment flow |
| Current fixed server found | UCloud `152.32.210.127` (`titan-forge` production-ish node), not Tencent Cloud |
| Jump host found | Aliyun `8.140.198.225` for RDS tunnel; not the Tencent bench box |

Conclusion: the original dynamic Tencent machine is probably gone. For this
gate, recreate a comparable Tencent CVM by spec rather than trying to reuse an
IP that was never recorded.

Recommended environment:

| Role | Spec | Notes |
|------|------|-------|
| Minimal single-box gate | Tencent Cloud S5/SA9e **8c16G**, Ubuntu 22.04 | Good for Phase 0-2 queue/runtime measurements; colocate Redis + workers + producer |
| Optional two-box stress | Server 8c16G + load box 16c32G, same VPC/AZ | Only needed if local producer CPU becomes a bottleneck |
| Existing fixed fallback | UCloud `152.32.210.127` | Useful only for smoke; not an apples-to-apples Tencent rerun |

Use the sysctl/ulimit checklist from
`/Users/admin/titan-forge/docs/BENCHMARK_LINUX.md` before running heavier
phases.

## Workload Contract

Use one common payload shape for both systems:

```json
{
  "task": "bench.echo",
  "kwargs": {
    "prompt": "minecraft grass block",
    "seed": 123,
    "width": 256,
    "height": 256,
    "sleep_ms": 0,
    "payload_bytes": 0
  },
  "metadata": {
    "producer_sent_ns": 123456789
  }
}
```

Expected result:

```json
{
  "ok": true,
  "task_id": "...",
  "echo": { "...": "..." },
  "worker_started_ns": 123456999,
  "worker_finished_ns": 123457999,
  "payload_bytes": 0
}
```

For texture-like payload tests, the worker returns a PNG-shaped or synthetic
base64 blob under:

```json
{
  "image_b64": "...",
  "format": "png"
}
```

## Feature Matrix

| Capability | Rustyme | Celery comparison |
|------------|---------|-------------------|
| Redis broker enqueue/dequeue | `LPUSH` / worker Redis source | Celery Redis broker |
| Result backend | result list JSON with `task_id/status/result/metadata` | Celery Redis result backend |
| Retry / timeout | envelope retries, timeout watchdog, DLQ | Celery retry / soft/hard time limits |
| Revoke / purge | admin API + cooperative skip | Celery revoke / purge |
| Group / chord | `group_id` + `chord_callback` envelope fields | Celery group / chord |
| Priority | high / normal / low weighted queues | Celery routing/priority |
| Python SDK | `rustyme-py` Celery-shaped API | native Celery API |
| Maquette progressive UX | independent task completion is natural | group/chord needs careful UI design |

## Metrics

All runs must write raw JSONL plus summary markdown.

Core metrics:

* throughput: tasks/sec
* enqueue → result latency: p50 / p95 / p99 / max
* enqueue → worker start latency, if worker stamps `worker_started_ns`
* result payload bytes
* error count / timeout count / duplicate result count / missing result count
* CPU/RSS snapshot if running on the Tencent CVM
* **CPU-time/task**: sum worker `utime+stime` before/after via
  `/proc/<pid>/stat`; for Celery, sum master + children
* **RSS/task proxy**: steady-state RSS divided by configured concurrency, plus
  total worker RSS for operational footprint
* Redis ops / memory if available (`INFO commandstats`, `INFO memory`) before
  and after each run

Reliability metrics:

* stale foreign result behavior
* worker kill/restart behavior
* Redis restart behavior
* group/chord callback completion rate
* revoke accuracy
* DLQ/retry behavior

Fairness controls:

* Same host, same Redis, same queue/result cleanup (`FLUSHALL`) before each run.
* Same worker concurrency where meaningful (`4` first, then `1/8` for scaling).
* Warm-up run before measured run; raw warm-up artifacts can be kept but excluded
  from primary tables.
* Same payload byte count and same result field shape.
* Celery must get at least two reasonable worker-pool configurations:
  `prefork` and `threads`. `gevent/eventlet` may be skipped only if the report
  explains why they do not match Maquette's sync/blocking workload.
* Rustyme must report which hook path was used: built-in EchoHook vs Lua hook vs
  real texgen Lua hook. These are not interchangeable.

## Phases

### Phase 0 — Harness + Environment

Goal: no performance claim yet; prove both systems can run the same task shape.

Tasks:

1. Create report directory:
   `maquette/reports/rustyme-vs-celery-2026-05/`
2. Run `bench_rustyme_vs_celery.py env-report`.
3. Bring up Redis locally or on the Tencent CVM.
4. Run Rustyme `bench.echo` worker using the Lua echo hook.
5. Run Celery worker with equivalent `bench.echo` task.

Exit criteria:

* Rustyme 1 task submit/result works.
* Celery 1 task submit/result works.
* Both write raw JSONL artifacts.

### Phase 1 — Feature Parity Smoke

Runs:

| Case | N | Expected |
|------|---:|----------|
| single task | 1 | result returned |
| small batch | 100 | 100/100 returned |
| Lua parity | 100 | Rustyme task actually executes `echo.lua`, not fallback EchoHook |
| payload 64 KiB | 100 | result with synthetic payload |
| payload 256 KiB | 100 | result with synthetic payload |
| fail/retry | 20 | retry count visible |
| timeout | 20 | timeout / DLQ visible |
| revoke | 20 | revoked tasks skipped |
| group/chord | 4, 12, 64 | callback exactly once |

### Phase 2 — Basic Performance

Runs:

| Workers | Tasks | Payload | Notes |
|---------|------:|---------|-------|
| 1 | 1,000 | no-op | baseline overhead |
| 4 | 5,000 | no-op | normal small run |
| 8 | 10,000 | no-op | stress without payload |
| 4 | 1,000 | 64 KiB synthetic | mid-size result |
| 4 | 1,000 | 256 KiB synthetic | texture-like result |
| 4 | 100 | sleep 2-8s | Fal-like long task |

For every Phase 2 row, collect:

* raw JSONL
* summary JSON
* worker `/proc` CPU deltas
* worker RSS before/after
* Redis memory before/after

### Phase 3 — Failure Recovery

Runs:

* result key preloaded with stale foreign replies
* worker `kill -9` mid-run
* Redis restart mid-run
* group with one failed child
* concurrent revoke/purge while producers are submitting
* Rustyme result list preloaded with stale foreign replies
* Celery result backend containing old task IDs (document behavior; direct
  equivalence may not exist because Celery result backend is keyed by task id)

### Phase 4 — Maquette Workload

Runs:

* one slot `Generate texture`
* 12-slot `Generate all`
* Rustyme independent tasks vs Rustyme chord variant
* Celery group/chord variant
* CPU lane and Fal-like sleep lane

User-visible metrics:

* time-to-first-texture
* time-to-all-textures
* error clarity
* whether progressive preview is better with independent tasks than chord

This phase must include both:

* independent-task progressive UX
* group/chord fan-in UX

The final report should explicitly say which one is better for Maquette's D-1
preview loop.

## Go / No-Go

Continue investing in Rustyme if:

* no-op and 256 KiB payload throughput are at least **70% of Celery**
* p95 result latency is no more than **1.5x Celery** for both no-op and payload
* 10k-task run has zero lost tasks and zero stuck result consumers
* group/chord 100 repeated runs have zero missing or duplicate callback
* stale foreign result / worker crash / timeout cases recover cleanly
* CPU-time/task is no worse than **1.5x Celery** and RSS footprint is materially
  better or equal
* Maquette progressive UX is simpler or better on Rustyme
* long-request fan-out overhead is no worse than **1.5x Celery** at the same
  concurrency and request-duration distribution

Stop adding Rustyme-specific features if:

* p95 latency is over **2x Celery**
* any 10k run loses tasks, duplicates callbacks, or hangs result collection
* group/chord is unstable
* worker kill / Redis restart / timeout recovery loses results silently
* long-request fan-out adds materially worse tail latency than Celery after
  subtracting request time
* operational visibility is materially worse than Celery/Flower
* Maquette keeps accumulating queue glue code just to match Celery basics

## Required Scenario — Long-Request Fan-Out Overhead

This is the Maquette-critical workload: many palette slots fan out to workers,
each worker performs a long external request (Fal.ai-like HTTP), and the user
cares about **queue/framework overhead outside the external request itself**.

Do **not** compare only wall-clock `enqueue → result` when each task sleeps or
calls HTTP for seconds. That mostly measures the external service. Instead each
worker must stamp:

```json
{
  "worker_started_ns": 123,
  "request_started_ns": 234,
  "request_finished_ns": 345,
  "worker_finished_ns": 456
}
```

Derived metrics:

| Metric | Formula | Meaning |
|---|---|---|
| queue_wait_ms | `worker_started - producer_sent` | broker + scheduler + worker availability |
| request_ms | `request_finished - request_started` | external request duration (subtract this) |
| worker_overhead_ms | `(worker_finished - worker_started) - request_ms` | framework/hook overhead inside worker excluding request |
| result_return_ms | `producer_received - worker_finished` | result backend / result consumer overhead |
| end_to_end_ms | `producer_received - producer_sent` | user-visible total |
| non_request_ms | `end_to_end_ms - request_ms` | what we are actually comparing |

Required matrix:

| Backend | Worker mode | Concurrency | Tasks | Request model |
|---|---|---:|---:|---|
| Rustyme | LuaJIT hook | 4 | 100 | local HTTP endpoint sleeping 2s |
| Celery | prefork | 4 | 100 | same endpoint |
| Celery | threads | 4 | 100 | same endpoint |
| Rustyme | LuaJIT hook | 16 | 400 | same endpoint |
| Celery | prefork | 16 | 400 | same endpoint |
| Celery | threads | 16 | 400 | same endpoint |
| Rustyme | LuaJIT hook | 32 | 800 | same endpoint |
| Celery | threads or prefork | 32 | 800 | same endpoint |

Why a **local HTTP endpoint** instead of `sleep_ms` only:

* `sleep_ms` tests scheduler behavior, but not socket open/read/write overhead.
* A local endpoint with controlled sleep gives stable request duration while
  still exercising HTTP client behavior.
* Fal.ai internet variability is tested later as a Maquette workload smoke, not
  as the primary framework-overhead benchmark.

The local endpoint should return its own server-side timing fields so client and
server clocks can be sanity-checked. If clock sync is suspect, use client-side
`request_started_ns/request_finished_ns` as the subtractor and record server
timing only for diagnostics.

Pass/fail emphasis:

* p95 / p99 **non_request_ms**
* time-to-first-result under fan-out
* time-to-all-results under fan-out
* CPU-time/task while requests are in flight
* RSS at concurrency 16 and 32

Middle path:

* keep current Rustyme support
* add `TaskQueueProvider` trait
* support `RustymeProvider` and `CeleryProvider` behind env/config

## Report Layout

```text
maquette/reports/rustyme-vs-celery-2026-05/
├── README.md                 # live report index
├── env/
│   └── env-report.json
├── raw/
│   ├── rustyme-phase1-*.jsonl
│   └── celery-phase1-*.jsonl
├── summaries/
│   ├── phase1-feature-parity.md
│   ├── phase2-performance.md
│   ├── phase3-failure-recovery.md
│   └── phase4-maquette-workload.md
└── final.md
```

## Immediate Next Steps

1. Create the report directory and harness skeleton.
2. Decide whether to run first on local Mac or recreate Tencent CVM.
3. If Tencent: use the titan-forge benchmark spec, not a stale IP.
4. Run Phase 0 and Phase 1 before spending time on heavy Phase 2/3.
