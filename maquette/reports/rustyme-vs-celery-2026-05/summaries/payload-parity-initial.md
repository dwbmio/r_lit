# Payload Parity Initial Results

Status: **initial payload comparison**  
Date: 2026-05-03  
Host: Tencent CVM `152.136.54.186`, local Redis.

## Scenario

Each task returns a synthetic base64 payload shaped like Maquette texture
results:

```json
{
  "ok": true,
  "image_b64": "...",
  "format": "png",
  "payload_bytes": 65536
}
```

Concurrency target: 16  
Task count: 1000

## Results

| Payload | Backend | Mode | OK | Throughput | p50 | p95 | CPU ms/task | RSS after |
|---:|---|---|---:|---:|---:|---:|---:|---:|
| 64 KiB | Rustyme | LuaJIT per-worker VM | 1000/1000 | **1727.9/s** | **285.6ms** | **347.5ms** | **0.40** | 62.1 MB |
| 64 KiB | Celery | threads=16 | 1000/1000 | 573.5/s | 881.3ms | 1070.6ms | 1.76 | 51.1 MB |
| 64 KiB | Celery | prefork=16 | 1000/1000 | 562.3/s | 910.4ms | 1169.3ms | 2.07 | 677.5 MB |
| 256 KiB | Rustyme | LuaJIT per-worker VM | 1000/1000 | **450.3/s** | **1126.2ms** | **1422.7ms** | **1.93** | 203.0 MB |
| 256 KiB | Celery | threads=16 | 1000/1000 | 173.5/s | 2953.7ms | 3336.1ms | 5.12 | 56.0 MB |
| 256 KiB | Celery | prefork=16 | 1000/1000 | 168.9/s | 2923.1ms | 3822.2ms | 6.22 | 710.0 MB |

## Read

Payload-sized results are currently Rustyme's clearest advantage:

* 64 KiB: ~3.0x Celery throughput and ~3.1x lower p95 latency.
* 256 KiB: ~2.6x Celery throughput and ~2.3x lower p95 latency.
* CPU-time/task is materially lower for Rustyme in both payload sizes.
* Rustyme RSS grows with payload size (203 MB at 256 KiB), but stays far below
  Celery prefork. Celery threads has lower RSS but much worse throughput/latency.

## Caveats

* Synthetic base64 payload, not actual PNG decode/encode.
* Redis memory/commandstats not yet captured.
* No failure/retry path in this payload run.

## Artifacts

Raw:

* `../raw/rustyme-lua-payload64k-16x1000.jsonl`
* `../raw/celery-threads16-payload64k-1000.jsonl`
* `../raw/celery-prefork16-payload64k-1000.jsonl`
* `../raw/rustyme-lua-payload256k-16x1000.jsonl`
* `../raw/celery-threads16-payload256k-1000.jsonl`
* `../raw/celery-prefork16-payload256k-1000.jsonl`

Summaries:

* `rustyme-lua-payload64k-16x1000.json`
* `celery-threads16-payload64k-1000.json`
* `celery-prefork16-payload64k-1000.json`
* `rustyme-lua-payload256k-16x1000.json`
* `celery-threads16-payload256k-1000.json`
* `celery-prefork16-payload256k-1000.json`

## Interim Judgment

For texture-like payload results, Rustyme per-worker LuaJIT has a clear
performance and CPU-efficiency advantage over Celery in this environment. This
is a meaningful positive signal for Maquette's texgen pipeline, pending
group/chord and failure-recovery gates.
