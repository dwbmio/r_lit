# Rustyme vs Celery 2026-05 Report Index

Status: **active gate / initial no-op EchoHook data collected**  
Plan: `maquette/docs/handoff/rustyme-vs-celery-plan.md`

This report directory is the artifact store for the roadmap blocker
`v0.10-QA Queue Backend Gate`. Do **not** treat any single summary here as
the final recommendation. `summaries/phase0-2-initial.md` is an initial
runtime-overhead signal only.

## Environment Notes

Historical titan-forge Linux bench environment:

* Tencent Cloud SA9e.2XLARGE16
* AMD EPYC 9K85 Turin
* 8 vCPU / 16 GB RAM
* Ubuntu 22.04.5 LTS
* kernel 5.15
* archived under `/Users/admin/titan-forge/benches/linux-20260420/`

The historical Tencent CVM public IP was not preserved in docs,
transcripts, or `~/.ssh/known_hosts`; treat it as a released dynamic
machine. Recreate by spec when running the gate.

Known / used machines:

| Host | Role | Notes |
|------|------|-------|
| `152.136.54.186` | Tencent CVM for this gate | Ubuntu 22.04.5, EPYC 9K85, 8c16G |
| `152.32.210.127` | UCloud titan-forge node | Current ops docs / health targets; not Tencent |
| `8.140.198.225` | Aliyun jump/RDS tunnel | Not a bench host |

## Artifact Layout

```text
env/
raw/
summaries/
final.md
```

Create subdirectories when running the harness.

## Pending Runs

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 0 — harness + env | partial | env-report exists; Rustyme/Celery smoke exists |
| Phase 1 — feature parity | partial | single / 100 done for EchoHook + Celery prefork; Lua / retry / timeout / revoke / chord pending |
| Phase 2 — performance | partial | no-op EchoHook vs Celery prefork 100 / 1k / 5k done; payload / sleep / Celery threads / CPU-time pending |
| Phase 3 — failure recovery | pending | stale result, worker kill, Redis restart |
| Phase 4 — Maquette workload | pending | one slot / Generate all / progressive UX |

## Required Before Final Decision

Final recommendation must wait for all of these:

| Required class | Status |
|---|---|
| Rustyme built-in EchoHook no-op | partial done |
| Rustyme Lua hook no-op | partial — per-worker Lua VM validated on long HTTP and payload; no-op Lua 1k/5k still pending |
| Celery prefork no-op | partial done |
| Celery threads no-op | pending |
| 64 KiB / 256 KiB payload | **initial pass** — `payload-parity-initial.md` |
| group/chord correctness | **initial pass** — group=12 x20, larger/failure pending |
| stale result / worker kill / Redis restart / timeout / revoke | stale result smoke only; rest pending |
| Maquette workload replay | pending |
| CPU-time/task + RSS/task | partial — long HTTP and payload captured |
| long-request fan-out overhead (`non_request_ms`) | **initial pass after per-worker VM** — larger/failure pending |

Initial long-request artifact now exists:

* `summaries/long-request-fanout-initial.md`

This is not a pass for Rustyme's default Lua path. It records a critical red
flag and the root-cause validation:

* Celery threads/prefork both completed 100 × 2s local HTTP tasks with
  concurrency=4 in ~50s.
* Rustyme LuaJIT **shared VM** returned 1/100 in the primary run.
* A 4-task shared-VM Rustyme run completed in ~8s, proving serialization.
* Experimental `RUSTYME_LUA_ISOLATED_PER_CALL=1` restored concurrency (4 tasks
  in ~2s, 100 tasks in ~3.27s), validating the shared Lua VM mutex as the
  proximate cause. This is a diagnostic, not the production fix.
* A per-worker Lua VM prototype also restored correctness while preserving
  Celery-like `concurrency=4` semantics (4 tasks in ~2s, 100 tasks in ~50s).
  This is the recommended production direction.

Per-worker Lua matrix artifact:

* `summaries/per-worker-lua-matrix.md`

At concurrency 16/32, per-worker LuaJIT matches Celery wall time on fixed
2-second HTTP fan-out but uses materially less CPU-time/task and far less memory
than Celery prefork. This supports continuing the Rustyme evaluation, pending
payload, group/chord, and failure-recovery gates.

Payload parity artifact:

* `summaries/payload-parity-initial.md`

At 64 KiB and 256 KiB synthetic result payloads, Rustyme per-worker LuaJIT is
~2.6-3.0x higher throughput than Celery and lower CPU-time/task. This is the
strongest positive signal so far for Maquette texture result delivery.

Chord correctness artifact:

* `summaries/chord-correctness-initial.md`

Both Rustyme and Celery passed group=12 / 20-run and group=64 / 100-run callback
correctness. Rustyme's callback latency is lower in these smoke runs. Failure
semantics differ: Rustyme leaves the chord incomplete and surfaces the failed
child through DLQ/timeout, while Celery marks the chord result as failed
(`ChordError`). Rustyme needs an explicit failed-child policy before user-facing
chord workflows.

Initial local-disk IO fan-out artifact:

* `summaries/io-fanout-initial.md`

This tests 100 fan-out tasks writing/reading/deleting 16 MiB each. Rustyme
LuaJIT is reliable here (100/100) and beats Celery threads, but Celery prefork
is faster in the fair no-fsync comparison. The Rustyme red flag is therefore
specific to LuaJIT long HTTP fan-out so far, not all IO.

## Harness Commands

Environment report (already safe to run anywhere):

```sh
python3 maquette/scripts/bench_rustyme_vs_celery.py env-report \
  --out maquette/reports/rustyme-vs-celery-2026-05/env/env-report.json
```

Celery worker:

```sh
cd maquette/scripts
CELERY_BROKER_URL=redis://127.0.0.1:6379/0 \
CELERY_RESULT_BACKEND=redis://127.0.0.1:6379/1 \
celery -A celery_bench_worker:app worker --loglevel=INFO --concurrency=4
```

Celery run:

```sh
python3 maquette/scripts/bench_rustyme_vs_celery.py celery-run \
  --broker redis://127.0.0.1:6379/0 \
  --backend redis://127.0.0.1:6379/1 \
  --task bench.echo \
  --count 100 \
  --raw maquette/reports/rustyme-vs-celery-2026-05/raw/celery-smoke.jsonl \
  --summary maquette/reports/rustyme-vs-celery-2026-05/summaries/celery-smoke.json
```

Rustyme run (requires a Rustyme queue bound to an echo hook, e.g.
`rustyme-lua/examples/scripts/echo.lua`):

```sh
python3 maquette/scripts/bench_rustyme_vs_celery.py rustyme-run \
  --redis-url redis://127.0.0.1:6379/0 \
  --queue-key rustyme:bench:queue \
  --result-key rustyme:bench:result \
  --task bench.echo \
  --count 100 \
  --raw maquette/reports/rustyme-vs-celery-2026-05/raw/rustyme-smoke.jsonl \
  --summary maquette/reports/rustyme-vs-celery-2026-05/summaries/rustyme-smoke.json
```

## Decision Slot

Final recommendation goes in `final.md`:

* continue Rustyme
* stop Rustyme and move to Celery
* keep both behind `TaskQueueProvider`
