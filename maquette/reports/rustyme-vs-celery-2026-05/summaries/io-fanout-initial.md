# IO Fan-Out Initial Results

Status: **initial local-disk IO comparison**  
Date: 2026-05-03  
Host: Tencent CVM `152.136.54.186`, local Redis, local disk `/tmp`.

## Scenario

Each task:

1. writes `16 MiB` to a unique temp file,
2. flushes/closes,
3. reads the file back,
4. deletes it.

Primary comparison is **no fsync** because Lua standard `io` has no portable
fsync equivalent. A first Celery run with `fsync()` exists but is not used as
the fair primary row.

## Results

| Backend | Worker mode | fsync | Tasks | OK | Timeout | Wall | Throughput | p50 e2e | p95 e2e | p95 non-IO |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Rustyme | LuaJIT `io.open` | no | 100 | 100 | 0 | 0.84s | 119.5/s | 438.5ms | 807.1ms | 800.1ms |
| Celery | prefork=4 | no | 100 | 100 | 0 | 0.31s | 327.1/s | 120.9ms | 214.1ms | 205.3ms |
| Celery | threads=4 | no | 100 | 100 | 0 | 5.10s | 19.6/s | 1095.0ms | 5024.7ms | 5014.9ms |
| Celery | prefork=4 | yes | 100 | 100 | 0 | 4.30s | 23.7/s | 1961.6ms | 3962.1ms | 3780.2ms |
| Celery | threads=4 | yes | 100 | 100 | 0 | 5.68s | 17.9/s | 2194.3ms | 5348.4ms | 5166.2ms |

## Interpretation

For local disk IO without fsync:

* Rustyme + LuaJIT is **stable** (100/100) and much better than Celery threads.
* Celery prefork is still **~2.7x faster** than Rustyme LuaJIT on throughput
  for this specific task shape.
* The earlier Rustyme LuaJIT HTTP red flag does **not** generalize to all IO:
  Lua local file IO did complete reliably.

However, the Rustyme LuaJIT IO timing has a caveat:

* Lua hook uses `date +%s%3N` via `os.exec` to stamp `io_elapsed_ms`; that adds
  process-spawn overhead and millisecond granularity. E2E wall time and OK/timeout
  counts are reliable, but `io_ms/non_io_ms` inside Lua rows are less precise
  than Celery's Python `time.time_ns()` fields.

## Key Takeaway

Rustyme + LuaJIT does **not** show an advantage on this local-disk heavy IO test
against Celery prefork, but it is reliable and outperforms Celery threads. The
critical failure remains specifically in LuaJIT long HTTP fan-out, not generic
local file IO.

## Artifacts

Raw:

* `../raw/rustyme-lua-io16m-4x100.jsonl`
* `../raw/celery-prefork-io16m-nofsync-4x100.jsonl`
* `../raw/celery-threads-io16m-nofsync-4x100.jsonl`
* `../raw/celery-prefork-io16m-4x100.jsonl` (fsync reference)
* `../raw/celery-threads-io16m-4x100.jsonl` (fsync reference)

Summaries:

* `rustyme-lua-io16m-4x100.json`
* `celery-prefork-io16m-nofsync-4x100.json`
* `celery-threads-io16m-nofsync-4x100.json`
* `celery-prefork-io16m-4x100.json`
* `celery-threads-io16m-4x100.json`

## Next Follow-Up

To make the IO comparison stronger:

1. Add a monotonic timestamp builtin to Rustyme Lua (`time.now_ns`) or return
   worker-side timing from Rust, not `date` subprocess.
2. Test larger per-task IO sizes: 64 MiB and 256 MiB.
3. Add an optional fsync-like path for Rustyme via a Rust-native hook, because
   Lua standard IO cannot fairly compare durable writes.
