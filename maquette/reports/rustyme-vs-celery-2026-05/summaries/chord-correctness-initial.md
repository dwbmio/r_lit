# Chord Correctness Initial Results

Status: **initial correctness smoke**  
Date: 2026-05-03  
Host: Tencent CVM `152.136.54.186`, local Redis.

## Scenario

Run repeated chords:

```text
N × bench.echo → bench.summarize
```

Pass condition:

* callback fires exactly once for each group,
* callback result contains `results_count == 12`,
* no timeout.

## Results

| Backend | Group size | Runs | OK | Failed | Timeout | p50 callback | p95 callback | p99 callback |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Rustyme per-worker Lua | 12 | 20 | 20 | 0 | 0 | **1.64ms** | **1.70ms** | 2.29ms |
| Celery threads=16 | 12 | 20 | 20 | 0 | 0 | 14.99ms | 16.68ms | 60.87ms |
| Rustyme per-worker Lua | 64 | 100 | 100 | 0 | 0 | **5.85ms** | **8.42ms** | 8.58ms |
| Celery threads=16 | 64 | 100 | 100 | 0 | 0 | 65.54ms | 68.73ms | 83.31ms |

## Payload-Bearing Chord

Each child returned a 64 KiB synthetic `image_b64` payload. Raw logs used compact
mode so the repository stores counts and sizes rather than every full callback
body.

| Backend | Group size | Runs | Payload / child | OK | Timeout | p50 callback | p95 callback | max |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Rustyme per-worker Lua | 12 | 20 | 64 KiB | 20 | 0 | **42.91ms** | **45.59ms** | 57.07ms |
| Celery threads=16 | 12 | 20 | 64 KiB | 20 | 0 | 77.15ms | 90.20ms | 122.92ms |
| Rustyme per-worker Lua | 64 | 10 | 64 KiB | 10 | 0 | **305.25ms** | **353.98ms** | 353.98ms |
| Celery threads=16 | 64 | 10 | 64 KiB | 10 | 0 | 395.19ms | 455.57ms | 455.57ms |

## Failure-child Semantics

Injected one intentional failure (`fail_index=3`) in a group of 12 and ran 3
repeats.

| Backend | Group size | Runs | Callback behavior | Failure surface |
|---|---:|---:|---|---|
| Rustyme per-worker Lua | 12 | 3 | callback does **not** fire; producer times out waiting for callback | failed child lands in `rustyme:payload:queue:dlq`; group counter never reaches total |
| Celery threads=16 | 12 | 3 | summarize callback does **not** run; chord result becomes failure (`ChordError`) | callback AsyncResult is ready with failure |

After the Rustyme failed-child policy patch, the Rustyme row becomes:

| Backend | Group size | Runs | Callback behavior | Failure surface |
|---|---:|---:|---|---|
| Rustyme per-worker Lua + failed-child policy | 12 | 3 | callback fires immediately, `results_count=12` | failed child appears in `results` as `{ok:false,status:"DEAD",error,...}` and still lands in DLQ |

## Read

Both systems pass the successful chord correctness smoke.

Rustyme's callback latency is much lower in this tiny group=12 case. This is
plausible because Rustyme's chord bookkeeping is a small number of Redis
`HSET/HINCRBY/HSETNX/LPUSH` operations in the worker, while Celery's chord uses
its backend machinery. The group=64 / 100-repeat run is also stable for both.
Payload-bearing chord also passes on both; Rustyme remains lower latency in the
tested 64 KiB child-result fan-in cases.

Pre-policy failure-child semantics differed:

* Rustyme leaves the chord incomplete and relies on DLQ/timeout observability.
* Celery marks the chord callback result as failure (`ChordError`).

The Rustyme patch now implements the first policy: failed terminal children
increment group done and contribute an explicit failed result. This is better for
Maquette's progressive UI because a group never hangs until timeout just because
one slot failed.

## Artifacts

Raw:

* `../raw/rustyme-chord-g12-r20-r2.jsonl`
* `../raw/celery-chord-g12-r20.jsonl`
* `../raw/rustyme-chord-g64-r100.jsonl`
* `../raw/celery-chord-g64-r100.jsonl`
* `../raw/rustyme-chord-fail-g12-r3.jsonl`
* `../raw/celery-chord-fail-g12-r3.jsonl`
* `../raw/rustyme-chord-fail-policy-g12-r3.jsonl`
* `../raw/rustyme-chord-payload64k-g12-r20.jsonl`
* `../raw/celery-chord-payload64k-g12-r20.jsonl`
* `../raw/rustyme-chord-payload64k-g64-r10.jsonl`
* `../raw/celery-chord-payload64k-g64-r10.jsonl`

Summaries:

* `rustyme-chord-g12-r20-r2.json`
* `celery-chord-g12-r20.json`
* `rustyme-chord-g64-r100.json`
* `celery-chord-g64-r100.json`
* `rustyme-chord-fail-g12-r3.json`
* `celery-chord-fail-g12-r3.json`
* `rustyme-chord-fail-policy-g12-r3.json`
* `rustyme-chord-payload64k-g12-r20.json`
* `celery-chord-payload64k-g12-r20.json`
* `rustyme-chord-payload64k-g64-r10.json`
* `celery-chord-payload64k-g64-r10.json`

## Next

1. Run failure-recovery scenarios (worker kill, Redis restart, revoke) with
   group/chord in flight.
