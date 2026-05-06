# Phase 0-2 Initial Results — Rustyme vs Celery

Status: **initial smoke + no-op performance only**  
Date: 2026-05-03  
Machine: Tencent Cloud `152.136.54.186`, Ubuntu 22.04.5, AMD EPYC 9K85,
8 vCPU / 16 GiB RAM.

## Conclusion Guard

This file is **not** the final recommendation.

It answers only one narrow question:

> What is the queue/runtime overhead of Rustyme built-in EchoHook vs Celery
> prefork on no-op tasks on one Tencent CVM?

It does **not** answer:

* Rustyme Lua hook overhead
* texture-sized payload behavior
* group/chord reliability
* worker crash / Redis restart recovery
* timeout/retry/DLQ behavior
* revoke/purge behavior
* Maquette `Generate all` real UX
* long-request fan-out overhead (`non_request_ms`, subtracting request time)
* CPU-time-per-task energy proxy
* Celery thread-pool alternative

Roadmap decisions must wait for the full v0.10-QA gate in
`docs/handoff/rustyme-vs-celery-plan.md`.

## Fairness Controls

Held constant:

* same host
* same local Redis instance (`127.0.0.1:6379`)
* same producer harness
* same no-op task shape (`bench.echo`)
* same worker concurrency target: 4
* same submit-then-collect measurement model
* raw JSONL retained under `../raw/`

Important caveat:

* Rustyme result here is **Rustyme runtime + built-in EchoHook**, because the
  Lua bench group did not bind on the first pass. It is valid for queue/runtime
  overhead but must not be presented as "Rustyme Lua worker" performance.
* Celery result is `Celery 5.6.3 + prefork concurrency=4 + Redis broker/backend`
  with `celery_bench_worker.py`.
* No payload and no Fal-like sleep in this initial table. Payload / sleep /
  group/chord / failure recovery still need follow-up runs.

## Summary Table

| Backend | Run | OK | Timeout | Throughput | p50 | p95 | p99 | Worker RSS |
|---------|-----|---:|--------:|-----------:|----:|----:|----:|-----------:|
| Rustyme EchoHook | 100 | 100 | 0 | 10,071 task/s | 4.37 ms | 4.42 ms | 4.44 ms | ~25 MiB |
| Celery prefork | 100 r2 | 100 | 0 | 1,124 task/s | 28.75 ms | 37.68 ms | 38.39 ms | ~190-200 MiB total |
| Rustyme EchoHook | 1,000 | 1,000 | 0 | 12,159 task/s | 40.72 ms | 41.17 ms | 41.23 ms | ~25 MiB |
| Celery prefork | 1,000 | 1,000 | 0 | 1,912 task/s | 219.88 ms | 284.41 ms | 296.61 ms | ~190-200 MiB total |
| Rustyme EchoHook | 5,000 | 5,000 | 0 | 12,613 task/s | 196.63 ms | 202.24 ms | 202.89 ms | ~25 MiB |
| Celery prefork | 5,000 | 5,000 | 0 | 2,151 task/s | 1,164.70 ms | 1,483.68 ms | 1,506.04 ms | ~190-200 MiB total |

Worker RSS notes:

* Rustyme single process snapshot around the 1,000-task run:
  `RSS 25,548 → 25,728 KiB`.
* Celery prefork snapshot around the 1,000-task run:
  master ~45 MiB + four children ~38 MiB each, roughly 190-200 MiB total.

## Reliability Smoke

Rustyme stale foreign result regression:

* Preloaded `rustyme:bench:result` with a stale foreign task result.
* Submitted 10 valid tasks.
* Result: `10/10 ok`, `0 timeout`.
* Raw log recorded foreign result handling events:
  `{"event": "foreign", "task_id": "foreign-stale"}`.

This specifically exercises the prior RPUSH/BRPOP dead-loop class of bug on the
producer side. The current harness LPUSHes foreign replies back to the list head,
so progress continues.

## Initial Read

For no-op queue/runtime overhead on this Tencent CVM:

* Rustyme is about **5.8x** Celery throughput on the 5k run.
* Rustyme p95 is about **7.3x lower latency** on the 5k run.
* Rustyme worker memory footprint is roughly **1/7 to 1/8** of Celery prefork.

This is a strong early signal for performance and energy-per-task, but it is not
yet a final decision because:

1. Rustyme Lua hook parity is not yet measured.
2. Payload-heavy texture-like results are not yet measured.
3. Celery group/chord vs Rustyme group/chord is not yet measured.
4. Worker crash / Redis restart / timeout / revoke are not yet measured.
5. CPU time for workers needs a cleaner before/after collection method than `ps`
   snapshots.

## Next Required Runs

1. Fix Rustyme Lua bench binding so `bench.echo` uses `echo.lua`, then repeat
   100 / 1k / 5k.
2. Add Celery `threads` pool run (`--pool=threads --concurrency=4`) for no-op
   and payload. Prefork-only Celery is not enough for a fair claim.
3. Run synthetic payload tests (`payload_bytes=65536` and `262144`) after making
   Celery and Rustyme return exactly one comparable payload field.
4. Add a clean worker CPU accounting helper:
   * read `/proc/<pid>/stat` before/after,
   * sum master + children for Celery,
   * divide by completed task count.
5. Run group/chord parity:
   * Rustyme `group_id + chord_callback`
   * Celery `group/chord`
6. Run long-request fan-out overhead:
   * local HTTP endpoint sleeps 2s and returns timing metadata,
   * Rustyme LuaJIT hook and Celery worker both issue the same HTTP request,
   * compute `non_request_ms = (producer_received - producer_sent) - request_ms`,
   * report p50/p95/p99, time-to-first-result, time-to-all-results, CPU-time/task.
7. Run failure phase:
   * worker kill/restart
   * Redis restart
   * revoke
   * timeout/DLQ
8. Produce `final.md` only after all rows above have raw artifacts.

## Artifacts

Raw:

* `../raw/rustyme-echo-smoke-100.jsonl`
* `../raw/celery-smoke-100-r2.jsonl`
* `../raw/rustyme-echo-1000.jsonl`
* `../raw/celery-1000.jsonl`
* `../raw/rustyme-echo-5000.jsonl`
* `../raw/celery-5000.jsonl`
* `../raw/rustyme-stale-foreign-10.jsonl`

Summaries:

* `rustyme-echo-smoke-100.json`
* `celery-smoke-100-r2.json`
* `rustyme-echo-1000.json`
* `celery-1000.json`
* `rustyme-echo-5000.json`
* `celery-5000.json`
* `rustyme-stale-foreign-10.json`
