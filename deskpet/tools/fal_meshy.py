#!/usr/bin/env python3
"""Image -> rigged + animated GLB via fal.ai's hosted Meshy 6 model.

Why fal instead of Meshy direct: the Meshy free plan blocks task creation
(HTTP 402). fal.ai hosts the same `fal-ai/meshy/v6/image-to-3d` model with
pay-per-use billing, so it works without a paid Meshy subscription.

Auth: env FAL_KEY (format "<uuid>:<secret>").

Usage:
    python3 fal_meshy.py gen --image block.png --out ../assets/block.glb --action 0

`--action` is a Meshy animation-library preset id (0 = Idle). The script enables
rigging + animation and downloads the animated GLB (falls back to rigged/static
if animation/rigging is absent).
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

MODEL = "fal-ai/meshy/v6/image-to-3d"
SUBMIT = f"https://queue.fal.run/{MODEL}"


def fal_key() -> str:
    k = os.environ.get("FAL_KEY")
    if not k:
        sys.exit("ERROR: FAL_KEY not set (format <uuid>:<secret>)")
    return k


def headers(k: str) -> dict:
    return {"Authorization": f"Key {k}", "Content-Type": "application/json"}


def http_post(url: str, payload: dict, k: str) -> dict:
    req = urllib.request.Request(url, data=json.dumps(payload).encode(), method="POST")
    for a, b in headers(k).items():
        req.add_header(a, b)
    try:
        with urllib.request.urlopen(req, timeout=120) as r:
            return json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"POST {url} -> HTTP {e.code}: {e.read().decode(errors='replace')}") from e


def http_get(url: str, k: str) -> dict:
    req = urllib.request.Request(url)
    for a, b in headers(k).items():
        req.add_header(a, b)
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.loads(r.read().decode())


def download(url: str, dest: Path, retries: int = 4) -> None:
    last: Exception | None = None
    for i in range(retries):
        try:
            with urllib.request.urlopen(url, timeout=180) as r:
                dest.parent.mkdir(parents=True, exist_ok=True)
                dest.write_bytes(r.read())
            return
        except (urllib.error.URLError, ConnectionResetError, TimeoutError) as e:
            last = e
            w = 2 ** i
            print(f"  download retry {i + 1}/{retries} in {w}s ({e})", flush=True)
            time.sleep(w)
    raise RuntimeError(f"download failed: {last!r}")


def data_uri(p: Path) -> str:
    mime = mimetypes.guess_type(str(p))[0] or "image/png"
    return f"data:{mime};base64," + base64.b64encode(p.read_bytes()).decode()


def gen(image: Path, out: Path, action_id: int) -> None:
    k = fal_key()
    body = {
        "image_url": data_uri(image),
        "enable_rigging": True,
        "enable_animation": True,
        "animation_action_id": action_id,
    }
    print(f"submitting {image.name} (rig+anim action={action_id}) ...", flush=True)
    sub = http_post(SUBMIT, body, k)
    req_id = sub.get("request_id")
    status_url = sub["status_url"]
    response_url = sub["response_url"]
    print(f"  request_id={req_id}", flush=True)

    start = time.time()
    last = None
    while time.time() - start < 1200:
        st = http_get(status_url, k)
        status = st.get("status")
        if status != last:
            print(f"  status={status} pos={st.get('queue_position', '')}", flush=True)
            last = status
        if status == "COMPLETED":
            break
        if status in ("FAILED", "ERROR"):
            raise RuntimeError(f"task {status}: {json.dumps(st)[:800]}")
        time.sleep(6)
    else:
        raise RuntimeError("poll timeout 1200s")

    res = http_get(response_url, k)
    glb = res.get("animation_glb") or res.get("rigged_character_glb") or res.get("model_glb") or {}
    url = glb.get("url")
    which = (
        "animation" if res.get("animation_glb")
        else "rigged" if res.get("rigged_character_glb")
        else "static"
    )
    if not url:
        raise RuntimeError(f"no glb in result: {json.dumps(res)[:800]}")
    print(f"  result type={which}, downloading -> {out}", flush=True)
    download(url, out)
    print(f"  saved {out.stat().st_size / 1024:.0f} KB ({which})", flush=True)
    # Stash the full result JSON for reference (urls, rig_task_id, etc.).
    out.with_suffix(".result.json").write_text(json.dumps(res, indent=2))


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["gen"])
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--action", type=int, default=0, help="Meshy animation preset id (0=Idle)")
    args = ap.parse_args()
    gen(Path(args.image).resolve(), Path(args.out).resolve(), args.action)


if __name__ == "__main__":
    main()
