#!/usr/bin/env python3
"""Downscale the embedded texture(s) of a .glb in place (keeps a .full backup).

Meshy/fal models ship 2048x2048 PBR textures (~6MB PNG, ~16MB uncompressed in
VRAM). A desktop pet shown at ~150px doesn't need that — 512 cuts VRAM ~16x.

Rebuilds the binary buffer with 4-byte-aligned bufferViews and fixed offsets,
then rewrites the GLB container. Accessor offsets are relative to their
bufferView so they stay valid.

Usage:
    python3 glb_shrink_texture.py ../assets/block.glb --size 512
"""
from __future__ import annotations

import argparse
import io
import json
import struct
from pathlib import Path

from PIL import Image

JSON_CHUNK = 0x4E4F534A
BIN_CHUNK = 0x004E4942


def parse_glb(data: bytes):
    assert data[:4] == b"glTF", "not a GLB"
    off = 12
    gltf = None
    binb = None
    while off < len(data):
        clen, ctype = struct.unpack_from("<II", data, off)
        off += 8
        chunk = data[off : off + clen]
        off += clen
        if ctype == JSON_CHUNK:
            gltf = json.loads(chunk.decode())
        elif ctype == BIN_CHUNK:
            binb = chunk
    return gltf, binb


def shrink_png(png: bytes, size: int) -> bytes:
    img = Image.open(io.BytesIO(png))
    if max(img.size) <= size:
        return png  # already small enough
    img.thumbnail((size, size), Image.LANCZOS)
    out = io.BytesIO()
    img.save(out, format="PNG", optimize=True)
    return out.getvalue()


def repack(path: Path, size: int, out: Path | None = None) -> None:
    data = path.read_bytes()
    gltf, binb = parse_glb(data)
    images = gltf.get("images", [])
    bufviews = gltf["bufferViews"]

    # Map which bufferViews hold images, downscale them, and re-encode as PNG.
    # We always emit PNG, so fix the declared mimeType too — otherwise a model
    # whose texture was originally JPEG ends up with PNG bytes but a stale
    # "image/jpeg" mimeType, which fails to decode unless the `jpeg` feature is
    # on (and would then mis-decode the PNG bytes).
    new_image_data: dict[int, bytes] = {}
    for im in images:
        bvi = im.get("bufferView")
        if bvi is None:
            continue
        bv = bufviews[bvi]
        o = bv.get("byteOffset", 0)
        l = bv["byteLength"]
        new_image_data[bvi] = shrink_png(binb[o : o + l], size)
        im["mimeType"] = "image/png"

    # Rebuild the binary buffer, 4-byte aligned, fixing every bufferView offset.
    new_buf = bytearray()
    for i, bv in enumerate(bufviews):
        if i in new_image_data:
            blob = new_image_data[i]
        else:
            o = bv.get("byteOffset", 0)
            blob = binb[o : o + bv["byteLength"]]
        while len(new_buf) % 4 != 0:
            new_buf.append(0)
        bv["byteOffset"] = len(new_buf)
        bv["byteLength"] = len(blob)
        new_buf += blob

    gltf["buffers"][0]["byteLength"] = len(new_buf)

    json_bytes = json.dumps(gltf, separators=(",", ":")).encode()
    while len(json_bytes) % 4 != 0:
        json_bytes += b" "
    bin_bytes = bytes(new_buf)
    while len(bin_bytes) % 4 != 0:
        bin_bytes += b"\x00"

    total = 12 + 8 + len(json_bytes) + 8 + len(bin_bytes)
    out_bytes = bytearray()
    out_bytes += b"glTF" + struct.pack("<II", 2, total)
    out_bytes += struct.pack("<II", len(json_bytes), JSON_CHUNK) + json_bytes
    out_bytes += struct.pack("<II", len(bin_bytes), BIN_CHUNK) + bin_bytes

    if out is not None:
        # Read-from / write-to different files: no in-place backup needed.
        out.write_bytes(bytes(out_bytes))
        print(
            f"{path.name} -> {out.name}: {len(data) / 1024 / 1024:.2f}MB -> "
            f"{len(out_bytes) / 1024 / 1024:.2f}MB (texture -> {size}px)"
        )
        return
    backup = path.with_suffix(".full.glb")
    if not backup.exists():
        backup.write_bytes(data)
    path.write_bytes(bytes(out_bytes))
    print(
        f"{path.name}: {len(data) / 1024 / 1024:.2f}MB -> {len(out_bytes) / 1024 / 1024:.2f}MB "
        f"(texture -> {size}px, backup {backup.name})"
    )


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("glb", nargs="+")
    ap.add_argument("--size", type=int, default=512)
    ap.add_argument("--out", help="write to this path instead of in-place (single input)")
    args = ap.parse_args()
    out = Path(args.out).resolve() if args.out else None
    for g in args.glb:
        repack(Path(g).resolve(), args.size, out)


if __name__ == "__main__":
    main()
