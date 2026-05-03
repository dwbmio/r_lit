# Failure Recovery Initial Results

Status: **initial worker-kill recovery smoke**  
Date: 2026-05-04  
Host: Tencent CVM `152.136.54.186`, local Redis, local sleep HTTP endpoint.

## Scenario

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

## Read

Both systems recovered from a worker-process kill and returned all 100 results.

Rustyme completed faster in this smoke. Its Redis processing-list recovery
replayed in-flight tasks after restart and left the processing list empty.
Celery also recovered with the reliability config above, but took longer in this
specific run.

## Caveats

* This covers worker kill only, not Redis restart.
* This uses one run per backend; repeat count is needed before final gate.
* Celery behavior depends heavily on `acks_late` / `reject_on_worker_lost` /
  prefetch settings. The report should always state those knobs.

## Artifacts

Raw:

* `../raw/rustyme-recovery-worker-kill-100.jsonl`
* `../raw/celery-recovery-worker-kill-100.jsonl`

Summaries:

* `rustyme-recovery-worker-kill-100.json`
* `celery-recovery-worker-kill-100.json`

## Next

1. Repeat worker-kill recovery 5-10 times.
2. Redis restart mid-run.
3. Timeout/retry/DLQ.
4. Revoke/purge during producer activity.
