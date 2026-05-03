# Failure Recovery Initial Results

Status: **initial worker-kill + Redis-restart recovery smoke**  
Date: 2026-05-04  
Host: Tencent CVM `152.136.54.186`, local Redis, local sleep HTTP endpoint.

## Scenario A — Worker Kill

100 tasks, each performs:

```text
GET http://127.0.0.1:18080/sleep?ms=2000
```

Procedure:

1. Start worker with concurrency=16.
2. Start producer / result collector.
3. Wait ~5 seconds.
4. Kill worker process.
5. Wait ~2 seconds.
6. Restart worker with same queue config.
7. Wait for producer to finish.

Celery used reliability-oriented config in `celery_bench_worker.py`:

```python
task_acks_late = True
task_reject_on_worker_lost = True
worker_prefetch_multiplier = 1
```

## Results

| Backend | OK | Failed | Timeout | Wall | p50 | p95 | p99 |
|---|---:|---:|---:|---:|---:|---:|---:|
| Rustyme per-worker Lua | 100 | 0 | 0 | ~14.18s | 12.05s | 14.17s | 14.17s |
| Celery threads=16 reliable config | 100 | 0 | 0 | ~25.10s | 14.05s | 22.06s | 25.04s |

## Scenario B — Redis Restart

Procedure:

1. Start worker with concurrency=16.
2. Start producer / result collector.
3. Wait ~5 seconds.
4. Restart `redis-server`.
5. Wait ~3 seconds.
6. Restart worker so Redis clients reconnect cleanly.
7. Wait for producer to finish.

| Backend | OK | Failed | Timeout | Wall | p50 | p95 | p99 | Observed transient |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Rustyme per-worker Lua | 100 | 0 | 0 | ~20.01s | 18.03s | 20.00s | 20.00s | producer saw `Connection closed by server` once |
| Celery threads=16 reliable config | 100 | 0 | 0 | ~26.23s | 15.18s | 23.19s | 26.16s | producer saw backend `Connection refused` once |

## Read

Both systems recovered from a worker-process kill and from a Redis restart,
returning all 100 results in these initial smokes.

Rustyme completed faster in this smoke. Its Redis processing-list recovery
replayed in-flight tasks after restart and left the processing list empty.
Celery also recovered with the reliability config above, but took longer in this
specific run.

## Caveats

* This uses one run per backend per scenario; repeat count is needed before final gate.
* Celery behavior depends heavily on `acks_late` / `reject_on_worker_lost` /
  prefetch settings. The report should always state those knobs.
* Redis persistence mode is the stock package config. This test restarts Redis
  service; it does not yet test crash-without-save or data-loss durability.

## Artifacts

Raw:

* `../raw/rustyme-recovery-worker-kill-100.jsonl`
* `../raw/celery-recovery-worker-kill-100.jsonl`
* `../raw/rustyme-recovery-redis-restart-100.jsonl`
* `../raw/celery-recovery-redis-restart-100.jsonl`

Summaries:

* `rustyme-recovery-worker-kill-100.json`
* `celery-recovery-worker-kill-100.json`
* `rustyme-recovery-redis-restart-100.json`
* `celery-recovery-redis-restart-100.json`

## Next

1. Repeat worker-kill and Redis-restart recovery 5-10 times.
2. Timeout/retry/DLQ.
3. Revoke/purge during producer activity.
