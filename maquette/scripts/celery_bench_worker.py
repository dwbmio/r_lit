#!/usr/bin/env python3
"""Minimal Celery worker for Rustyme-vs-Celery benchmark parity.

Run:

    cd maquette/scripts
    CELERY_BROKER_URL=redis://127.0.0.1:6379/0 \
    CELERY_RESULT_BACKEND=redis://127.0.0.1:6379/1 \
    celery -A celery_bench_worker:app worker \
        --loglevel=INFO --concurrency=4 --pool=prefork

The task name is `bench.echo`, matching the Rustyme benchmark payload
contract in `rustyme-vs-celery-plan.md`.
"""

from __future__ import annotations

import base64
import json
import os
from pathlib import Path
import time
from typing import Any
from urllib.request import urlopen

from celery import Celery


BROKER = os.environ.get("CELERY_BROKER_URL", "redis://127.0.0.1:6379/0")
BACKEND = os.environ.get("CELERY_RESULT_BACKEND", "redis://127.0.0.1:6379/1")

app = Celery("maquette_bench", broker=BROKER, backend=BACKEND)
app.conf.update(
    task_acks_late=True,
    task_reject_on_worker_lost=True,
    worker_prefetch_multiplier=1,
)


@app.task(name="bench.echo", bind=True)
def bench_echo(self, **kwargs: Any) -> dict[str, Any]:
    if kwargs.get("fail"):
        raise RuntimeError(f"intentional bench failure: {kwargs.get('seed')}")
    started_ns = time.time_ns()
    sleep_ms = int(kwargs.get("sleep_ms") or 0)
    if sleep_ms > 0:
        time.sleep(sleep_ms / 1000.0)

    payload_bytes = int(kwargs.get("payload_bytes") or 0)
    image_b64 = ""
    if payload_bytes > 0:
        # Deterministic enough for payload-size benchmarking; the exact
        # bytes are irrelevant because this is not an image-quality test.
        image_b64 = base64.b64encode(b"x" * payload_bytes).decode("ascii")

    return {
        "ok": True,
        "task_id": self.request.id,
        "echo": kwargs,
        "worker_started_ns": started_ns,
        "worker_finished_ns": time.time_ns(),
        "payload_bytes": payload_bytes,
        "image_b64": image_b64,
        "format": "png" if image_b64 else None,
    }


@app.task(name="bench.long_request", bind=True)
def bench_long_request(self, **kwargs: Any) -> dict[str, Any]:
    worker_started_ns = time.time_ns()
    url = kwargs.get("request_url") or "http://127.0.0.1:18080/sleep?ms=2000"
    timeout = float(kwargs.get("request_timeout_secs") or 30.0)
    request_started_ns = time.time_ns()
    with urlopen(url, timeout=timeout) as resp:  # noqa: S310 - controlled local bench URL
        body = resp.read()
    request_finished_ns = time.time_ns()
    server = json.loads(body.decode("utf-8"))
    worker_finished_ns = time.time_ns()
    return {
        "ok": True,
        "task_id": self.request.id,
        "request_url": url,
        "worker_started_ns": worker_started_ns,
        "request_started_ns": request_started_ns,
        "request_finished_ns": request_finished_ns,
        "worker_finished_ns": worker_finished_ns,
        "request_elapsed_ms": server.get("request_elapsed_ms")
        or ((request_finished_ns - request_started_ns) / 1_000_000.0),
        "server": server,
        "echo": kwargs,
    }


@app.task(name="bench.io_file", bind=True)
def bench_io_file(self, **kwargs: Any) -> dict[str, Any]:
    worker_started_ns = time.time_ns()
    io_dir = Path(kwargs.get("io_dir") or "/tmp/rustyme-vs-celery-io")
    io_dir.mkdir(parents=True, exist_ok=True)
    mib = int(kwargs.get("io_mib") or 16)
    fsync = bool(kwargs.get("io_fsync") or False)
    chunk = b"x" * (1024 * 1024)
    path = io_dir / f"celery-{self.request.id}.bin"

    io_started_ns = time.time_ns()
    with path.open("wb") as f:
        for _ in range(mib):
            f.write(chunk)
        f.flush()
        if fsync:
            os.fsync(f.fileno())
    total = 0
    with path.open("rb") as f:
        while True:
            data = f.read(1024 * 1024)
            if not data:
                break
            total += len(data)
    path.unlink(missing_ok=True)
    io_finished_ns = time.time_ns()
    worker_finished_ns = time.time_ns()
    return {
        "ok": True,
        "task_id": self.request.id,
        "worker_started_ns": worker_started_ns,
        "io_started_ns": io_started_ns,
        "io_finished_ns": io_finished_ns,
        "worker_finished_ns": worker_finished_ns,
        "io_elapsed_ms": (io_finished_ns - io_started_ns) / 1_000_000.0,
        "bytes": total,
        "echo": kwargs,
    }


@app.task(name="bench.summarize", bind=True)
def bench_summarize(self, results: list[Any] | None = None, **kwargs: Any) -> dict[str, Any]:
    results = results or []
    return {
        "ok": True,
        "task_id": self.request.id,
        "group_id": kwargs.get("group_id"),
        "total": len(results),
        "results_count": len(results),
        "results": results,
        "echo": kwargs,
        "worker_finished_ns": time.time_ns(),
    }
