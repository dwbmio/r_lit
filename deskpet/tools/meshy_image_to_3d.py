#!/usr/bin/env python3
"""Meshy.ai Image-to-3D — turn a single-character image into a GLB.

Auth: env MESHY_API_KEY (msy_xxx). Get one at https://www.meshy.ai/settings/api

Usage:
    python3 meshy_image_to_3d.py balance
    python3 meshy_image_to_3d.py gen --image block.png --out ../assets/block.glb

The local image is sent inline as a base64 data URI (no external host needed).
Polls the async task until SUCCEEDED, then downloads the GLB.
"""
from __future__ import annotations

import argparse
import base64
import json
import mimetypes
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

BASE_URL = "https://api.meshy.ai"


def headers(key: str) -> dict:
    return {"Authorization": f"Bearer {key}", "Content-Type": "application/json"}


def http_post(url: str, payload: dict, key: str) -> dict:
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), method="POST")
    for k, v in headers(key).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"POST {url} -> HTTP {e.code}: {e.read().decode(errors='replace')}") from e


def http_get(url: str, key: str) -> dict:
    req = urllib.request.Request(url)
    for k, v in headers(key).items():
        req.add_header(k, v)
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode())


def download(url: str, dest: Path, retries: int = 4) -> None:
    last: Exception | None = None
    for i in range(retries):
        try:
            with urllib.request.urlopen(url, timeout=120) as r:
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(r.read())
            return
        except (urllib.error.URLError, ConnectionResetError, TimeoutError) as e:
            last = e
            w = 2 ** i
            print(f"  download retry {i + 1}/{retries} in {w}s ({e})", flush=True)
            time.sleep(w)
    raise RuntimeError(f"download failed: {last!r}")


def balance(key: str) -> int:
    return http_get(f"{BASE_URL}/openapi/v1/balance", key).get("balance", -1)


def data_uri(path: Path) -> str:
    mime = mimetypes.guess_type(str(path))[0] or "image/png"
    b64 = base64.b64encode(path.read_bytes()).decode()
    return f"data:{mime};base64,{b64}"


def poll(task_id: str, key: str, label: str, interval: int = 5, max_s: int = 600) -> dict:
    url = f"{BASE_URL}/openapi/v1/image-to-3d/{task_id}"
    start = time.time()
    last = -1
    while time.time() - start < max_s:
        t = http_get(url, key)
        st = t.get("status")
        pr = t.get("progress", 0)
        if pr != last:
            print(f"  [{label}] status={st} progress={pr}%", flush=True)
            last = pr
        if st == "SUCCEEDED":
            return t
        if st in ("FAILED", "CANCELED"):
            raise RuntimeError(f"task {st}: {json.dumps(t.get('task_error', {}))}")
        time.sleep(interval)
    raise RuntimeError(f"poll timeout {max_s}s")


def gen(key: str, image: Path, out: Path) -> None:
    print(f"balance before: {balance(key)} credits")
    payload = {
        "image_url": data_uri(image),
        "should_texture": True,
        "should_remesh": True,
        "enable_pbr": False,
    }
    print(f"creating image-to-3d task from {image} ...")
    resp = http_post(f"{BASE_URL}/openapi/v1/image-to-3d", payload, key)
    task_id = resp["result"]
    print(f"  task_id={task_id}")
    task = poll(task_id, key, image.stem)
    glb = task["result"]["model_urls"]["glb"]
    print(f"  downloading -> {out}")
    download(glb, out)
    print(f"  saved {out.stat().st_size / 1024:.0f} KB")
    print(f"balance after: {balance(key)} credits")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["balance", "gen"])
    ap.add_argument("--image")
    ap.add_argument("--out")
    args = ap.parse_args()

    key = os.environ.get("MESHY_API_KEY")
    if not key:
        print("ERROR: export MESHY_API_KEY=msy_xxx")
        sys.exit(1)

    if args.cmd == "balance":
        print(f"balance: {balance(key)} credits")
        return
    if not args.image or not args.out:
        print("ERROR: gen needs --image and --out")
        sys.exit(1)
    gen(key, Path(args.image).resolve(), Path(args.out).resolve())


if __name__ == "__main__":
    main()
