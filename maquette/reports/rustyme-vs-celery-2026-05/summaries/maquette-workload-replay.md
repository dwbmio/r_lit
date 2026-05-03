# Maquette Workload Replay

Status: **initial workload replay**  
Date: 2026-05-04  
Host: Tencent CVM `152.136.54.186`, local Redis, local sleep HTTP endpoint.

## Scenario

Replay Maquette's D-1 texture-generation shape:

* **one slot**: 1 task
* **Generate all**: 12-slot fan-out
* each task calls `GET /sleep?ms=2000` to mimic Fal-like long request
* each task returns synthetic `image_b64` payload
* payload sizes: 64 KiB and 256 KiB
* worker concurrency: 12

## Results

### 12-slot Generate all

| Payload | Backend | OK | Wall | p50 | p95 | non-request p95 | CPU ms/task | RSS |
|---:|---|---:|---:|---:|---:|---:|---:|---:|
| 64 KiB | Rustyme per-worker Lua | 12/12 | ~2.04s | 2008.0ms | 2011.1ms | 17.6ms | 1.67 | 43.8 MB |
| 64 KiB | Celery threads=12 | 12/12 | ~2.09s | 2028.8ms | 2030.0ms | ~15ms | 4.17 | 63.3 MB |
| 64 KiB | Celery prefork=12 | 12/12 | ~2.06s | 2014.6ms | 2017.0ms | ~21ms | n/a | n/a |
| 256 KiB | Rustyme per-worker Lua | 12/12 | ~2.04s | 2016.8ms | 2018.5ms | 17.6ms | 2.50 | 70.1 MB |
| 256 KiB | Celery threads=12 | 12/12 | ~2.09s | 2015.0ms | 2016.2ms | 15.1ms | 5.83 | 79.6 MB |
| 256 KiB | Celery prefork=12 | 12/12 | ~2.09s | 2018.5ms | 2023.0ms | 21.1ms | n/a | n/a |

### One slot

All backends complete one-slot in about the external request duration:

| Payload | Backend | OK | Latency |
|---:|---|---:|---:|
| 64 KiB | Rustyme per-worker Lua | 1/1 | ~2005ms |
| 64 KiB | Celery threads=12 | 1/1 | ~2046ms |
| 64 KiB | Celery prefork=12 | 1/1 | ~2046ms |
| 256 KiB | Rustyme per-worker Lua | 1/1 | ~2008ms |
| 256 KiB | Celery threads=12 | 1/1 | ~2764ms (single-run outlier) |
| 256 KiB | Celery prefork=12 | 1/1 | ~2053ms |

## Read

For Maquette's 12-slot Generate all shape, wall time is dominated by the 2s
external request and all correct implementations finish in roughly 2 seconds at
concurrency=12.

The differentiator is efficiency:

* Rustyme uses less CPU-time/task than Celery threads in the measured 12-slot
  runs.
* Rustyme RSS is lower than Celery threads for 12-slot 64/256 KiB payloads.
* Celery prefork has comparable wall-time but is known from the larger fan-out
  matrix to have much higher RSS; CPU/RSS for the tiny 12-slot prefork rows was
  not captured cleanly in this run.

This confirms the queue-gate synthetic findings under the actual Maquette shape:
Rustyme does not hurt user-visible Generate all latency and retains an efficiency
advantage.

## Caveats

* Local sleep endpoint, not real Fal.ai.
* One run per row.
* Prefork CPU/RSS missing for this tiny replay; use the long-request matrix for
  prefork resource comparison.
* This does not exercise Maquette GUI code directly; it replays the task-queue
  shape.

## Artifacts

Raw / summaries:

* `rustyme-maquette-c1-p65536`
* `rustyme-maquette-c12-p65536`
* `rustyme-maquette-c1-p262144`
* `rustyme-maquette-c12-p262144`
* `celery-threads-maquette-c1-p65536`
* `celery-threads-maquette-c12-p65536`
* `celery-threads-maquette-c1-p262144`
* `celery-threads-maquette-c12-p262144`
* `celery-prefork-maquette-c1-p65536`
* `celery-prefork-maquette-c12-p65536`
* `celery-prefork-maquette-c1-p262144`
* `celery-prefork-maquette-c12-p262144`

## Interim Judgment

The Maquette replay does not reveal a Celery advantage in the target shape.
Rustyme per-worker Lua is competitive on wall time and better on measured
efficiency. The remaining gate is now less about performance and more about final
reliability semantics (especially DLQ/result-surface behavior) and production
polish.
