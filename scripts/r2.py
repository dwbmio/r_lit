#!/usr/bin/env python3
"""Tiny R2 (S3-compatible) helper used by scripts/repatch_hfrog.sh and as a
local fallback when aws-cli is broken.

Usage:
    R2_ENDPOINT=... R2_ACCESS_KEY_ID=... R2_SECRET_ACCESS_KEY=... \\
    R2_BUCKET=prod-gamesci-lite \\
        python3 scripts/r2.py ls   r_lit/
        python3 scripts/r2.py cp   ./install.sh r_lit/bulk_upload/install.sh \\
            --content-type "text/x-shellscript; charset=utf-8" \\
            --cache-control "public, max-age=300"
        python3 scripts/r2.py rm   r_lit/old_path/

Run from any system Python ≥ 3.8 with `boto3` installed; we deliberately do
NOT depend on the user's broken pyenv/awscli.
"""

import argparse
import os
import sys
from pathlib import Path

import boto3
from botocore.client import Config


def _client():
    endpoint = os.environ["R2_ENDPOINT"]
    return boto3.client(
        "s3",
        endpoint_url=endpoint,
        aws_access_key_id=os.environ["R2_ACCESS_KEY_ID"],
        aws_secret_access_key=os.environ["R2_SECRET_ACCESS_KEY"],
        region_name=os.environ.get("R2_REGION", "auto"),
        config=Config(signature_version="s3v4"),
    )


def cmd_ls(args):
    bucket = os.environ["R2_BUCKET"]
    s3 = _client()
    paginator = s3.get_paginator("list_objects_v2")
    seen = 0
    for page in paginator.paginate(Bucket=bucket, Prefix=args.prefix):
        for o in page.get("Contents", []):
            print(f"{o['Size']:>12d}  {o['LastModified'].isoformat()}  {o['Key']}")
            seen += 1
    if not seen:
        print(f"(no objects under {args.prefix!r})", file=sys.stderr)


def cmd_cp(args):
    bucket = os.environ["R2_BUCKET"]
    s3 = _client()
    src = Path(args.src)
    if not src.is_file():
        sys.exit(f"src not found or not a file: {src}")
    extra = {}
    if args.content_type:
        extra["ContentType"] = args.content_type
    if args.cache_control:
        extra["CacheControl"] = args.cache_control
    s3.upload_file(str(src), bucket, args.key, ExtraArgs=extra)
    size = src.stat().st_size
    print(f"  ↑ s3://{bucket}/{args.key} ({size} bytes)")


def cmd_rm(args):
    bucket = os.environ["R2_BUCKET"]
    s3 = _client()
    paginator = s3.get_paginator("list_objects_v2")
    deleted = 0
    for page in paginator.paginate(Bucket=bucket, Prefix=args.prefix):
        objs = [{"Key": o["Key"]} for o in page.get("Contents", [])]
        if not objs:
            continue
        s3.delete_objects(Bucket=bucket, Delete={"Objects": objs})
        for o in objs:
            print(f"  ✗ {o['Key']}")
        deleted += len(objs)
    print(f"deleted {deleted} object(s)")


def main():
    p = argparse.ArgumentParser()
    sub = p.add_subparsers(dest="cmd", required=True)

    ls = sub.add_parser("ls")
    ls.add_argument("prefix", nargs="?", default="")
    ls.set_defaults(func=cmd_ls)

    cp = sub.add_parser("cp")
    cp.add_argument("src")
    cp.add_argument("key", help="destination object key (no s3:// prefix)")
    cp.add_argument("--content-type")
    cp.add_argument("--cache-control")
    cp.set_defaults(func=cmd_cp)

    rm = sub.add_parser("rm")
    rm.add_argument("prefix")
    rm.set_defaults(func=cmd_rm)

    args = p.parse_args()
    for need in ("R2_ENDPOINT", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "R2_BUCKET"):
        if not os.environ.get(need):
            sys.exit(f"missing env: {need}")
    args.func(args)


if __name__ == "__main__":
    main()
