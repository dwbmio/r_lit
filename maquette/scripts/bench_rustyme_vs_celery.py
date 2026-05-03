#!/usr/bin/env python3
"""Rustyme vs Celery benchmark harness (Phase 0-2).

This script is intentionally small and explicit. It does not start Redis,
Rustyme, Celery, or any worker process for you; it submits a common task
shape to an already-running backend, collects results, and writes raw JSONL
plus a summary JSON.

Useful first commands:

    python maquette/scripts/bench_rustyme_vs_celery.py env-report \
        --out maquette/reports/rustyme-vs-celery-2026-05/env/env-report.json

    python maquette/scripts/bench_rustyme_vs_celery.py rustyme-run \
        --redis-url redis://127.0.0.1:6379/0 \
        --queue-key rustyme:bench:queue \
        --result-key rustyme:bench:result \
        --task bench.echo \
        --count 100 \
        --raw maquette/reports/rustyme-vs-celery-2026-05/raw/rustyme-smoke.jsonl \
        --summary maquette/reports/rustyme-vs-celery-2026-05/summaries/rustyme-smoke.json

    python maquette/scripts/bench_rustyme_vs_celery.py celery-run \
        --broker redis://127.0.0.1:6379/0 \
        --backend redis://127.0.0.1:6379/1 \
        --task bench.echo \
        --count 100 \
        --raw maquette/reports/rustyme-vs-celery-2026-05/raw/celery-smoke.jsonl \
        --summary maquette/reports/rustyme-vs-celery-2026-05/summaries/celery-smoke.json

Dependencies:

* env-report: stdlib only
* rustyme-run: `pip install redis`
* celery-run: `pip install celery redis`
"""

from __future__ import annotations

import argparse
import base64
import json
import os
import platform
import random
import socket
import statistics
import subprocess
import sys
import time
import uuid
from dataclasses import dataclass
from pathlib import Path
from typing import Any


def now_ns() -> int:
    return time.perf_counter_ns()


def wall_ns() -> int:
    return time.time_ns()


def ensure_parent(path: str | Path) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)


def read_pid_file(path: str | None) -> int | None:
    if not path:
        return None
    try:
        return int(Path(path).read_text(encoding="utf-8").strip())
    except Exception:
        return None


def proc_stat(pid: int) -> tuple[int, int, int, int] | None:
    """Return (ppid, utime_ticks, stime_ticks, rss_pages)."""
    try:
        raw = Path(f"/proc/{pid}/stat").read_text(encoding="utf-8")
    except Exception:
        return None
    # comm is wrapped in parentheses and may contain spaces; split after last ).
    try:
        _left, right = raw.rsplit(") ", 1)
        fields = right.split()
        ppid = int(fields[1])
        utime = int(fields[11])
        stime = int(fields[12])
        rss = int(fields[21])
        return ppid, utime, stime, rss
    except Exception:
        return None


def descendants(root_pid: int) -> list[int]:
    ppid_map: dict[int, int] = {}
    for p in Path("/proc").iterdir():
        if not p.name.isdigit():
            continue
        pid = int(p.name)
        stat = proc_stat(pid)
        if stat:
            ppid_map[pid] = stat[0]
    children: dict[int, list[int]] = {}
    for pid, ppid in ppid_map.items():
        children.setdefault(ppid, []).append(pid)
    out: list[int] = []
    stack = [root_pid]
    seen: set[int] = set()
    while stack:
        pid = stack.pop()
        if pid in seen:
            continue
        seen.add(pid)
        if proc_stat(pid):
            out.append(pid)
        stack.extend(children.get(pid, []))
    return sorted(out)


def worker_snapshot(root_pid_file: str | None) -> dict[str, Any] | None:
    root_pid = read_pid_file(root_pid_file)
    if not root_pid:
        return None
    hz = os.sysconf(os.sysconf_names.get("SC_CLK_TCK", "SC_CLK_TCK"))
    page_size = os.sysconf("SC_PAGE_SIZE")
    rows = []
    for pid in descendants(root_pid):
        stat = proc_stat(pid)
        if not stat:
            continue
        _ppid, utime, stime, rss_pages = stat
        rows.append(
            {
                "pid": pid,
                "cpu_ticks": utime + stime,
                "rss_bytes": rss_pages * page_size,
            }
        )
    return {
        "root_pid": root_pid,
        "pids": rows,
        "total_cpu_ticks": sum(r["cpu_ticks"] for r in rows),
        "total_rss_bytes": sum(r["rss_bytes"] for r in rows),
        "hz": hz,
    }


def add_worker_metrics(
    summary: dict[str, Any],
    before: dict[str, Any] | None,
    after: dict[str, Any] | None,
    completed: int,
) -> None:
    if not before or not after:
        return
    delta_ticks = after["total_cpu_ticks"] - before["total_cpu_ticks"]
    hz = after["hz"] or before["hz"]
    cpu_seconds = delta_ticks / hz if hz else None
    summary["worker_process"] = {
        "before": before,
        "after": after,
        "cpu_seconds_delta": cpu_seconds,
        "cpu_ms_per_completed_task": (cpu_seconds * 1000.0 / completed)
        if cpu_seconds is not None and completed > 0
        else None,
        "rss_bytes_after": after["total_rss_bytes"],
    }


def percentile(values: list[float], p: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    idx = min(len(ordered) - 1, max(0, int(round((p / 100.0) * (len(ordered) - 1)))))
    return ordered[idx]


def summarize(latencies_ms: list[float], *, count: int, ok: int, failed: int, timeout: int) -> dict[str, Any]:
    return {
        "count": count,
        "ok": ok,
        "failed": failed,
        "timeout": timeout,
        "latency_ms": {
            "min": min(latencies_ms) if latencies_ms else None,
            "p50": percentile(latencies_ms, 50),
            "p95": percentile(latencies_ms, 95),
            "p99": percentile(latencies_ms, 99),
            "max": max(latencies_ms) if latencies_ms else None,
            "mean": statistics.fmean(latencies_ms) if latencies_ms else None,
        },
    }


def add_timing_summary(summary: dict[str, Any], rows: list[dict[str, float]]) -> None:
    if not rows:
        return
    keys = sorted({k for row in rows for k in row})
    out: dict[str, Any] = {}
    for key in keys:
        vals = [row[key] for row in rows if key in row]
        out[key] = {
            "min": min(vals),
            "p50": percentile(vals, 50),
            "p95": percentile(vals, 95),
            "p99": percentile(vals, 99),
            "max": max(vals),
            "mean": statistics.fmean(vals),
        }
    summary["timing_ms"] = out


def make_kwargs(
    i: int,
    *,
    sleep_ms: int,
    payload_bytes: int,
    io_mib: int = 0,
    io_dir: str | None = None,
    io_fsync: bool = False,
    request_url: str | None = None,
    fail: bool = False,
) -> dict[str, Any]:
    payload = ""
    if payload_bytes > 0:
        payload = base64.b64encode(os.urandom(payload_bytes)).decode("ascii")
    data = {
        "prompt": f"bench prompt {i}",
        "seed": i,
        "width": 256,
        "height": 256,
        "sleep_ms": sleep_ms,
        "payload_bytes": payload_bytes,
        "payload_b64": payload,
        "io_mib": io_mib,
        "io_fsync": io_fsync,
    }
    if io_dir:
        data["io_dir"] = io_dir
    if request_url:
        data["request_url"] = request_url
    if fail:
        data["fail"] = True
    return data


def timing_metrics(
    *,
    result: dict[str, Any],
    producer_sent_wall_ns: int,
    producer_received_wall_ns: int,
) -> dict[str, float]:
    metrics: dict[str, float] = {}
    request_ms = result.get("request_elapsed_ms")
    if isinstance(request_ms, (int, float)):
        metrics["request_ms"] = float(request_ms)
    io_ms = result.get("io_elapsed_ms")
    if isinstance(io_ms, (int, float)):
        metrics["io_ms"] = float(io_ms)
    worker_started = result.get("worker_started_ns")
    worker_finished = result.get("worker_finished_ns")
    request_started = result.get("request_started_ns")
    request_finished = result.get("request_finished_ns")
    if isinstance(worker_started, int):
        metrics["queue_wait_ms"] = (worker_started - producer_sent_wall_ns) / 1_000_000.0
    if isinstance(worker_finished, int):
        metrics["result_return_ms"] = (producer_received_wall_ns - worker_finished) / 1_000_000.0
    if isinstance(worker_started, int) and isinstance(worker_finished, int) and "request_ms" in metrics:
        metrics["worker_overhead_ms"] = (
            (worker_finished - worker_started) / 1_000_000.0
        ) - metrics["request_ms"]
    if isinstance(request_started, int) and isinstance(request_finished, int):
        metrics["client_request_ms"] = (request_finished - request_started) / 1_000_000.0
    end_to_end = (producer_received_wall_ns - producer_sent_wall_ns) / 1_000_000.0
    metrics["end_to_end_wall_ms"] = end_to_end
    if "request_ms" in metrics:
        metrics["non_request_ms"] = end_to_end - metrics["request_ms"]
    if "io_ms" in metrics:
        metrics["non_io_ms"] = end_to_end - metrics["io_ms"]
    return metrics


def env_report(args: argparse.Namespace) -> None:
    report = {
        "created_at": int(time.time()),
        "hostname": socket.gethostname(),
        "platform": platform.platform(),
        "python": sys.version,
        "machine": platform.machine(),
        "processor": platform.processor(),
        "cwd": os.getcwd(),
        "git": git_info(),
        "env": {
            key: os.environ.get(key)
            for key in [
                "MAQUETTE_HFROG_BASE_URL",
                "MAQUETTE_RUSTYME_REDIS_URL",
                "RUSTYME_REDIS_URL",
                "CELERY_BROKER_URL",
                "CELERY_RESULT_BACKEND",
            ]
            if os.environ.get(key)
        },
    }
    ensure_parent(args.out)
    Path(args.out).write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    print(args.out)


def git_info() -> dict[str, Any]:
    def run(cmd: list[str]) -> str | None:
        try:
            return subprocess.check_output(cmd, stderr=subprocess.DEVNULL, text=True).strip()
        except Exception:
            return None

    return {
        "root": run(["git", "rev-parse", "--show-toplevel"]),
        "head": run(["git", "rev-parse", "HEAD"]),
        "branch": run(["git", "branch", "--show-current"]),
        "dirty": run(["git", "status", "--short"]),
    }


@dataclass
class RustymeRun:
    redis_url: str
    queue_key: str
    result_key: str
    task: str
    count: int
    sleep_ms: int
    payload_bytes: int
    io_mib: int
    io_dir: str | None
    request_url: str | None
    timeout_secs: float
    raw: str
    summary: str


def rustyme_run(args: argparse.Namespace) -> None:
    try:
        import redis  # type: ignore
    except ImportError as exc:
        raise SystemExit("rustyme-run needs `pip install redis`") from exc

    cfg = RustymeRun(
        redis_url=args.redis_url,
        queue_key=args.queue_key,
        result_key=args.result_key,
        task=args.task,
        count=args.count,
        sleep_ms=args.sleep_ms,
        payload_bytes=args.payload_bytes,
        io_mib=args.io_mib,
        io_dir=args.io_dir,
        request_url=args.request_url,
        timeout_secs=args.timeout_secs,
        raw=args.raw,
        summary=args.summary,
    )
    r = redis.Redis.from_url(cfg.redis_url, decode_responses=True)
    ensure_parent(cfg.raw)
    ensure_parent(cfg.summary)

    sent: dict[str, int] = {}
    with Path(cfg.raw).open("w", encoding="utf-8") as raw:
        t0 = now_ns()
        worker_before = worker_snapshot(args.worker_root_pid_file)
        sent_wall: dict[str, int] = {}
        timing_rows: list[dict[str, float]] = []
        for i in range(cfg.count):
            task_id = str(uuid.uuid4())
            producer_sent_wall_ns = wall_ns()
            envelope = {
                "id": task_id,
                "task": cfg.task,
                "args": [],
                "kwargs": make_kwargs(
                    i,
                    sleep_ms=cfg.sleep_ms,
                    payload_bytes=cfg.payload_bytes,
                    io_mib=cfg.io_mib,
                    io_dir=cfg.io_dir,
                    io_fsync=args.io_fsync,
                    request_url=cfg.request_url,
                    fail=False,
                ),
                "retries": 0,
                "max_retries": 0,
                "priority": "normal",
                "metadata": {
                    "producer": "bench_rustyme_vs_celery.py",
                    "producer_sent_wall_ns": producer_sent_wall_ns,
                },
            }
            sent[task_id] = now_ns()
            sent_wall[task_id] = producer_sent_wall_ns
            r.lpush(cfg.queue_key, json.dumps(envelope, separators=(",", ":")))
            raw.write(json.dumps({"event": "sent", "task_id": task_id, "i": i}) + "\n")

        ok = failed = timeout = 0
        latencies: list[float] = []
        remaining = set(sent)
        deadline = time.time() + cfg.timeout_secs
        while remaining and time.time() < deadline:
            try:
                item = r.brpop(cfg.result_key, timeout=1)
            except Exception as exc:
                raw.write(
                    json.dumps(
                        {"event": "redis_error", "op": "brpop", "error": str(exc)[:500]},
                        ensure_ascii=False,
                    )
                    + "\n"
                )
                time.sleep(0.2)
                continue
            if not item:
                continue
            _, payload = item
            try:
                data = json.loads(payload)
            except json.JSONDecodeError:
                raw.write(json.dumps({"event": "bad_json", "payload": payload[:512]}) + "\n")
                continue
            task_id = data.get("task_id")
            if task_id not in remaining:
                # Preserve foreign/stale results for diagnosis, then put them
                # back at the head so another consumer can still see them. This
                # mirrors the LPUSH fix that prevented the RPUSH dead loop.
                r.lpush(cfg.result_key, payload)
                raw.write(json.dumps({"event": "foreign", "task_id": task_id}) + "\n")
                time.sleep(0.01)
                continue
            remaining.remove(task_id)
            received_wall_ns = wall_ns()
            elapsed_ms = (now_ns() - sent[task_id]) / 1_000_000.0
            latencies.append(elapsed_ms)
            result_obj = data.get("result") if isinstance(data.get("result"), dict) else {}
            timing = timing_metrics(
                result=result_obj,
                producer_sent_wall_ns=sent_wall[task_id],
                producer_received_wall_ns=received_wall_ns,
            )
            if timing:
                timing_rows.append(timing)
            status = data.get("status")
            if status == "SUCCESS":
                ok += 1
            else:
                failed += 1
            raw.write(
                json.dumps(
                    {
                        "event": "result",
                        "task_id": task_id,
                        "status": status,
                        "elapsed_ms": elapsed_ms,
                            "timing": timing,
                        "result_size": len(payload),
                    },
                    ensure_ascii=False,
                )
                + "\n"
            )

        timeout = len(remaining)
        t1 = now_ns()
        worker_after = worker_snapshot(args.worker_root_pid_file)
        summary = summarize(latencies, count=cfg.count, ok=ok, failed=failed, timeout=timeout)
        summary["backend"] = "rustyme"
        summary["queue_key"] = cfg.queue_key
        summary["result_key"] = cfg.result_key
        summary["wall_ms"] = (t1 - t0) / 1_000_000.0
        summary["throughput_tps"] = ok / ((t1 - t0) / 1_000_000_000.0) if t1 > t0 else None
        summary["remaining"] = sorted(remaining)[:20]
        add_timing_summary(summary, timing_rows)
        add_worker_metrics(summary, worker_before, worker_after, ok)
        Path(cfg.summary).write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
        print(json.dumps(summary, ensure_ascii=False, indent=2))


def celery_run(args: argparse.Namespace) -> None:
    try:
        from celery import Celery  # type: ignore
    except ImportError as exc:
        raise SystemExit("celery-run needs `pip install celery redis`") from exc

    app = Celery("maquette_bench", broker=args.broker, backend=args.backend)
    ensure_parent(args.raw)
    ensure_parent(args.summary)
    latencies: list[float] = []
    ok = failed = timeout = 0

    with Path(args.raw).open("w", encoding="utf-8") as raw:
        pending: list[tuple[str, int, Any]] = []
        sent_wall: dict[str, int] = {}
        timing_rows: list[dict[str, float]] = []
        t0 = now_ns()
        worker_before = worker_snapshot(args.worker_root_pid_file)
        for i in range(args.count):
            sent_ns = now_ns()
            sent_wall_ns = wall_ns()
            res = app.send_task(
                args.task,
                kwargs=make_kwargs(
                    i,
                    sleep_ms=args.sleep_ms,
                    payload_bytes=args.payload_bytes,
                    io_mib=args.io_mib,
                    io_dir=args.io_dir,
                    io_fsync=args.io_fsync,
                    request_url=args.request_url,
                    fail=False,
                ),
            )
            sent_wall[res.id] = sent_wall_ns
            pending.append((res.id, sent_ns, res))
            raw.write(json.dumps({"event": "sent", "task_id": res.id, "i": i}) + "\n")

        pending_by_id = {task_id: (sent_ns, res) for task_id, sent_ns, res in pending}
        deadline = time.time() + args.timeout_secs
        while pending_by_id and time.time() < deadline:
            progressed = False
            for task_id, (sent_ns, res) in list(pending_by_id.items()):
                try:
                    ready = res.ready()
                except Exception as exc:
                    raw.write(
                        json.dumps(
                            {
                                "event": "backend_error",
                                "op": "ready",
                                "task_id": task_id,
                                "error": str(exc)[:500],
                            },
                            ensure_ascii=False,
                        )
                        + "\n"
                    )
                    time.sleep(0.2)
                    continue
                if not ready:
                    continue
                progressed = True
                try:
                    data = res.get(timeout=0, propagate=False)
                    received_wall_ns = wall_ns()
                    elapsed_ms = (now_ns() - sent_ns) / 1_000_000.0
                    latencies.append(elapsed_ms)
                    timing = timing_metrics(
                        result=data if isinstance(data, dict) else {},
                        producer_sent_wall_ns=sent_wall[task_id],
                        producer_received_wall_ns=received_wall_ns,
                    )
                    if timing:
                        timing_rows.append(timing)
                    ok += 1
                    raw.write(
                        json.dumps(
                            {
                                "event": "result",
                                "task_id": task_id,
                                "elapsed_ms": elapsed_ms,
                                "timing": timing,
                                "result_size": len(json.dumps(data, default=str)),
                            },
                            ensure_ascii=False,
                        )
                        + "\n"
                    )
                except Exception as exc:  # Celery raises many backend-specific errors.
                    failed += 1
                    raw.write(json.dumps({"event": "error", "task_id": task_id, "error": str(exc)}) + "\n")
                finally:
                    pending_by_id.pop(task_id, None)
            if not progressed:
                time.sleep(0.005)
        timeout = len(pending_by_id)
        for task_id in pending_by_id:
            raw.write(json.dumps({"event": "timeout", "task_id": task_id}) + "\n")
        t1 = now_ns()
        worker_after = worker_snapshot(args.worker_root_pid_file)

    summary = summarize(latencies, count=args.count, ok=ok, failed=failed, timeout=timeout)
    summary["backend"] = "celery"
    summary["broker"] = args.broker
    summary["backend_url"] = args.backend
    summary["wall_ms"] = (t1 - t0) / 1_000_000.0
    summary["throughput_tps"] = ok / ((t1 - t0) / 1_000_000_000.0) if t1 > t0 else None
    add_timing_summary(summary, timing_rows)
    add_worker_metrics(summary, worker_before, worker_after, ok)
    Path(args.summary).write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))


def compact_result_event(event: str, data: dict[str, Any], gid: str | None = None) -> dict[str, Any]:
    result = data.get("result") if isinstance(data.get("result"), dict) else {}
    echo = result.get("echo") if isinstance(result.get("echo"), dict) else {}
    results = None
    if isinstance(result.get("results"), list):
        results = result["results"]
    elif isinstance(echo.get("results"), list):
        results = echo["results"]
    return {
        "event": event,
        "gid": gid,
        "task_id": data.get("task_id"),
        "status": data.get("status"),
        "result_task_id": result.get("task_id"),
        "group_id": result.get("group_id") or echo.get("group_id"),
        "results_count": len(results) if results is not None else result.get("results_count"),
        "payload_bytes": result.get("payload_bytes"),
        "result_size": len(json.dumps(data, default=str)),
        "failed_results": sum(1 for r in results or [] if isinstance(r, dict) and r.get("ok") is False),
    }


def rustyme_chord_run(args: argparse.Namespace) -> None:
    try:
        import redis  # type: ignore
    except ImportError as exc:
        raise SystemExit("rustyme-chord-run needs `pip install redis`") from exc

    r = redis.Redis.from_url(args.redis_url, decode_responses=True)
    ensure_parent(args.raw)
    ensure_parent(args.summary)
    ok = failed = timeout = 0
    callback_latencies: list[float] = []
    rows: list[dict[str, Any]] = []
    with Path(args.raw).open("w", encoding="utf-8") as raw:
        for run_idx in range(args.runs):
            gid = f"bench-{uuid.uuid4()}"
            total = args.group_size
            counter_key = f"rustyme:group:{gid}:counter"
            results_key = f"rustyme:group:{gid}:results"
            pipe = r.pipeline(transaction=False)
            pipe.hset(counter_key, "total", total)
            pipe.expire(counter_key, args.group_ttl_secs)
            pipe.expire(results_key, args.group_ttl_secs)
            pipe.execute()

            sent_wall_ns = wall_ns()
            child_ids = []
            for i in range(total):
                task_id = str(uuid.uuid4())
                child_ids.append(task_id)
                envelope = {
                    "id": task_id,
                    "task": args.task,
                    "args": [],
                    "kwargs": make_kwargs(
                        i,
                        sleep_ms=args.sleep_ms,
                        payload_bytes=args.payload_bytes,
                        io_mib=0,
                        fail=(args.fail_index == i),
                    ),
                    "retries": 0,
                    "max_retries": 0,
                    "priority": "normal",
                    "group_id": gid,
                    "chord_callback": {
                        "task": args.callback_task,
                        "queue_key": args.queue_key,
                        "kwargs": {"bench_run_idx": run_idx},
                    },
                    "metadata": {"producer_sent_wall_ns": sent_wall_ns, "kind": "rustyme-chord"},
                }
                r.lpush(args.queue_key, json.dumps(envelope, separators=(",", ":")))
            raw.write(json.dumps({"event": "sent_group", "gid": gid, "total": total}) + "\n")

            callback_seen = False
            deadline = time.time() + args.timeout_secs
            while time.time() < deadline:
                item = r.brpop(args.result_key, timeout=1)
                if not item:
                    continue
                _, payload = item
                try:
                    data = json.loads(payload)
                except json.JSONDecodeError:
                    raw.write(json.dumps({"event": "bad_json", "payload": payload[:512]}) + "\n")
                    continue
                result = data.get("result") if isinstance(data.get("result"), dict) else {}
                echo = result.get("echo") if isinstance(result.get("echo"), dict) else {}
                if args.compact_raw:
                    raw.write(
                        json.dumps(
                            compact_result_event("result", data, gid),
                            ensure_ascii=False,
                            default=str,
                        )
                        + "\n"
                    )
                else:
                    raw.write(
                        json.dumps({"event": "result", "gid": gid, "data": data}, ensure_ascii=False)
                        + "\n"
                    )
                if echo.get("group_id") == gid or result.get("group_id") == gid:
                    callback_seen = True
                    received_wall_ns = wall_ns()
                    elapsed_ms = (received_wall_ns - sent_wall_ns) / 1_000_000.0
                    callback_latencies.append(elapsed_ms)
                    count = result.get("results_count")
                    row = {
                        "gid": gid,
                        "elapsed_ms": elapsed_ms,
                        "results_count": count,
                        "expected": total,
                        "ok": count == total,
                    }
                    rows.append(row)
                    if count == total:
                        ok += 1
                    else:
                        failed += 1
                    break
            if not callback_seen:
                timeout += 1
                rows.append({"gid": gid, "ok": False, "timeout": True, "expected": total})

    summary = summarize(callback_latencies, count=args.runs, ok=ok, failed=failed, timeout=timeout)
    summary.update(
        {
            "backend": "rustyme",
            "kind": "chord",
            "group_size": args.group_size,
            "runs": args.runs,
            "rows": rows[:20],
        }
    )
    Path(args.summary).write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))


def celery_chord_run(args: argparse.Namespace) -> None:
    try:
        from celery import Celery, chord, group  # type: ignore
    except ImportError as exc:
        raise SystemExit("celery-chord-run needs `pip install celery redis`") from exc

    app = Celery("maquette_bench", broker=args.broker, backend=args.backend)
    ensure_parent(args.raw)
    ensure_parent(args.summary)
    ok = failed = timeout = 0
    callback_latencies: list[float] = []
    rows: list[dict[str, Any]] = []
    with Path(args.raw).open("w", encoding="utf-8") as raw:
        for run_idx in range(args.runs):
            sent_wall_ns = wall_ns()
            header = group(
                app.signature(
                    args.task,
                    kwargs=make_kwargs(
                        i,
                        sleep_ms=args.sleep_ms,
                        payload_bytes=args.payload_bytes,
                        io_mib=0,
                        fail=(args.fail_index == i),
                    ),
                )
                for i in range(args.group_size)
            )
            callback = app.signature(args.callback_task, kwargs={"bench_run_idx": run_idx})
            res = chord(header)(callback)
            raw.write(json.dumps({"event": "sent_chord", "callback_id": res.id}) + "\n")
            deadline = time.time() + args.timeout_secs
            while time.time() < deadline and not res.ready():
                time.sleep(0.01)
            if not res.ready():
                timeout += 1
                rows.append({"callback_id": res.id, "ok": False, "timeout": True})
                continue
            data = res.get(timeout=0, propagate=False)
            received_wall_ns = wall_ns()
            elapsed_ms = (received_wall_ns - sent_wall_ns) / 1_000_000.0
            callback_latencies.append(elapsed_ms)
            count = data.get("results_count") if isinstance(data, dict) else None
            row = {
                "callback_id": res.id,
                "elapsed_ms": elapsed_ms,
                "results_count": count,
                "expected": args.group_size,
                "ok": count == args.group_size,
            }
            rows.append(row)
            if args.compact_raw:
                raw.write(
                    json.dumps(
                        compact_result_event("callback", data, None),
                        ensure_ascii=False,
                        default=str,
                    )
                    + "\n"
                )
            else:
                raw.write(
                    json.dumps({"event": "callback", "data": data}, ensure_ascii=False, default=str)
                    + "\n"
                )
            if count == args.group_size:
                ok += 1
            else:
                failed += 1

    summary = summarize(callback_latencies, count=args.runs, ok=ok, failed=failed, timeout=timeout)
    summary.update(
        {
            "backend": "celery",
            "kind": "chord",
            "group_size": args.group_size,
            "runs": args.runs,
            "rows": rows[:20],
        }
    )
    Path(args.summary).write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps(summary, ensure_ascii=False, indent=2))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="cmd", required=True)

    p = sub.add_parser("env-report")
    p.add_argument("--out", required=True)
    p.set_defaults(func=env_report)

    p = sub.add_parser("rustyme-run")
    p.add_argument("--redis-url", default=os.environ.get("RUSTYME_REDIS_URL", "redis://127.0.0.1:6379/0"))
    p.add_argument("--queue-key", default="rustyme:bench:queue")
    p.add_argument("--result-key", default="rustyme:bench:result")
    p.add_argument("--task", default="bench.echo")
    p.add_argument("--count", type=int, default=100)
    p.add_argument("--sleep-ms", type=int, default=0)
    p.add_argument("--payload-bytes", type=int, default=0)
    p.add_argument("--io-mib", type=int, default=0)
    p.add_argument("--io-dir")
    p.add_argument("--io-fsync", action="store_true")
    p.add_argument("--request-url")
    p.add_argument("--timeout-secs", type=float, default=30.0)
    p.add_argument("--worker-root-pid-file")
    p.add_argument("--raw", required=True)
    p.add_argument("--summary", required=True)
    p.set_defaults(func=rustyme_run)

    p = sub.add_parser("celery-run")
    p.add_argument("--broker", default=os.environ.get("CELERY_BROKER_URL", "redis://127.0.0.1:6379/0"))
    p.add_argument("--backend", default=os.environ.get("CELERY_RESULT_BACKEND", "redis://127.0.0.1:6379/1"))
    p.add_argument("--task", default="bench.echo")
    p.add_argument("--count", type=int, default=100)
    p.add_argument("--sleep-ms", type=int, default=0)
    p.add_argument("--payload-bytes", type=int, default=0)
    p.add_argument("--io-mib", type=int, default=0)
    p.add_argument("--io-dir")
    p.add_argument("--io-fsync", action="store_true")
    p.add_argument("--request-url")
    p.add_argument("--timeout-secs", type=float, default=30.0)
    p.add_argument("--worker-root-pid-file")
    p.add_argument("--raw", required=True)
    p.add_argument("--summary", required=True)
    p.set_defaults(func=celery_run)

    p = sub.add_parser("rustyme-chord-run")
    p.add_argument("--redis-url", default=os.environ.get("RUSTYME_REDIS_URL", "redis://127.0.0.1:6379/0"))
    p.add_argument("--queue-key", default="rustyme:payload:queue")
    p.add_argument("--result-key", default="rustyme:payload:result")
    p.add_argument("--task", default="bench.echo")
    p.add_argument("--callback-task", default="bench.summarize")
    p.add_argument("--group-size", type=int, default=12)
    p.add_argument("--runs", type=int, default=10)
    p.add_argument("--group-ttl-secs", type=int, default=3600)
    p.add_argument("--sleep-ms", type=int, default=0)
    p.add_argument("--payload-bytes", type=int, default=0)
    p.add_argument("--fail-index", type=int)
    p.add_argument("--compact-raw", action="store_true")
    p.add_argument("--timeout-secs", type=float, default=30.0)
    p.add_argument("--raw", required=True)
    p.add_argument("--summary", required=True)
    p.set_defaults(func=rustyme_chord_run)

    p = sub.add_parser("celery-chord-run")
    p.add_argument("--broker", default=os.environ.get("CELERY_BROKER_URL", "redis://127.0.0.1:6379/0"))
    p.add_argument("--backend", default=os.environ.get("CELERY_RESULT_BACKEND", "redis://127.0.0.1:6379/1"))
    p.add_argument("--task", default="bench.echo")
    p.add_argument("--callback-task", default="bench.summarize")
    p.add_argument("--group-size", type=int, default=12)
    p.add_argument("--runs", type=int, default=10)
    p.add_argument("--sleep-ms", type=int, default=0)
    p.add_argument("--payload-bytes", type=int, default=0)
    p.add_argument("--fail-index", type=int)
    p.add_argument("--compact-raw", action="store_true")
    p.add_argument("--timeout-secs", type=float, default=30.0)
    p.add_argument("--raw", required=True)
    p.add_argument("--summary", required=True)
    p.set_defaults(func=celery_chord_run)

    return parser


def main() -> None:
    parser = build_parser()
    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
