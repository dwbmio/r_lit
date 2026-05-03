#!/usr/bin/env python3
"""Local sleep HTTP endpoint for long-request fan-out benchmarks.

The point of this server is to make the "external request" part of a task
stable and measurable. Workers call `/sleep?ms=2000`; the server sleeps for
that duration and returns timing metadata. The benchmark harness then subtracts
`request_elapsed_ms` from end-to-end task latency to isolate queue/framework
overhead (`non_request_ms`).
"""

from __future__ import annotations

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse


class SleepHandler(BaseHTTPRequestHandler):
    server_version = "MaquetteBenchSleep/0.1"

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        received_ns = time.time_ns()
        parsed = urlparse(self.path)
        if parsed.path not in {"/sleep", "/health"}:
            self.send_error(404, "not found")
            return
        if parsed.path == "/health":
            self.write_json({"ok": True, "received_ns": received_ns})
            return

        qs = parse_qs(parsed.query)
        ms = int(qs.get("ms", ["0"])[0])
        ms = max(0, min(ms, 120_000))
        sleep_started_ns = time.time_ns()
        time.sleep(ms / 1000.0)
        sleep_finished_ns = time.time_ns()
        self.write_json(
            {
                "ok": True,
                "requested_ms": ms,
                "server_received_ns": received_ns,
                "sleep_started_ns": sleep_started_ns,
                "sleep_finished_ns": sleep_finished_ns,
                "request_elapsed_ms": (sleep_finished_ns - sleep_started_ns) / 1_000_000.0,
            }
        )

    def write_json(self, data: dict) -> None:
        body = json.dumps(data, separators=(",", ":")).encode("utf-8")
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, fmt: str, *args) -> None:  # noqa: ANN001
        # Keep benchmark logs clean; request counts are measured by harness raw
        # JSONL, not access logs.
        return


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=18080)
    args = parser.parse_args()
    server = ThreadingHTTPServer((args.host, args.port), SleepHandler)
    print(f"sleep server listening on http://{args.host}:{args.port}", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
