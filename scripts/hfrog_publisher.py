#!/usr/bin/env python3
"""hfrog_publisher — single source of truth for publishing artifacts.

Combines three previously-duplicated jobs into one tool used by both:

  * r_lit's GitHub Actions (`.github/workflows/release.yml`)
  * Every Rust pipeline in ci-all-in-one (`task/ci/pipeline/<proj>/Jenkinsfile.*`)

What it does for one (tool, version, [assets…]) tuple:

  1. POST /api/release/softwares    (description, install_command, install_script_url)
  2. POST /api/release/platforms    (one per asset target)
  3. POST /api/release/versions     (is_latest, release_notes, created_by)
  4. POST /api/release/releases     (per asset: file_size, checksum_sha256, source_type, download_url)
  5. (optional) UPDATE rb_softwares.{category_id, llms_txt, readme_url, install_*} via psycopg2
  6. (optional) Upload each asset to R2     (s3://<bucket>/<prefix>/<tool>/v<ver>/<basename>)
  7. (optional) Render install.sh template, upload to R2 stable + immutable paths

Every hfrog write is idempotent — duplicate-key 1002 / "already exists" responses
are reported as "= …" and treated as success. Failures emit `::error::` lines
suitable for GitHub Actions log grouping and exit non-zero only when the run as
a whole couldn't reach a consistent state.

Three CLI entry points; pick the one that matches the caller's stage:

  ./hfrog_publisher.py publish          # full sync (most callers want this)
  ./hfrog_publisher.py mirror           # R2 upload only
  ./hfrog_publisher.py patch-meta       # SQL-level field refresh (no API)

Run any of them with --help to see arguments. All paths and JSON values are read
from CLI flags or env vars (R2_*, HFROG_API, POSTGRES_HFROG_*) — no hidden
defaults that would silently change behaviour between callers.

Dependencies (pip install --user boto3; system python on macOS / runner has it):
  - boto3
  - psycopg2-binary (only if --postgres-url is passed)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Iterable

# ─── shared constants ──────────────────────────────────────────────────────────

DEFAULT_PLATFORM_DICT: dict[str, tuple[str, str, str]] = {
    # target_triple → (os, arch, display_name)
    "x86_64-unknown-linux-gnu":  ("linux",   "x86_64",  "Linux x86_64"),
    "aarch64-unknown-linux-gnu": ("linux",   "aarch64", "Linux ARM64"),
    "x86_64-unknown-linux-musl": ("linux",   "x86_64",  "Linux x86_64 (musl)"),
    "aarch64-unknown-linux-musl":("linux",   "aarch64", "Linux ARM64 (musl)"),
    "aarch64-apple-darwin":      ("macos",   "aarch64", "macOS ARM64 (Apple Silicon)"),
    "x86_64-apple-darwin":       ("macos",   "x86_64",  "macOS Intel"),
    "x86_64-pc-windows-msvc":    ("windows", "x86_64",  "Windows x86_64"),
    "aarch64-pc-windows-msvc":   ("windows", "aarch64", "Windows ARM64"),
}

DUPLICATE_PATTERNS = re.compile(
    r"already.*exist|AlreadyExist|duplicate key value", re.IGNORECASE
)

ANSI = {"r": "\033[0;31m", "g": "\033[0;32m", "y": "\033[1;33m", "0": "\033[0m"}


def _log(level: str, msg: str) -> None:
    color = {"info": "g", "warn": "y", "err": "r", "ok": "g"}.get(level, "0")
    print(f"{ANSI.get(color, '')}[{level}]{ANSI['0']} {msg}", flush=True)


def info(msg: str) -> None: _log("info", msg)
def warn(msg: str) -> None: _log("warn", msg)
def err(msg: str)  -> None: _log("err",  msg)


# ─── hfrog HTTP client ─────────────────────────────────────────────────────────

class HfrogClient:
    def __init__(self, api_base: str):
        self.api = api_base.rstrip("/")

    def post(self, endpoint: str, payload: dict[str, Any]) -> tuple[bool, str]:
        url = f"{self.api}{endpoint}"
        body = json.dumps(payload).encode()
        req = urllib.request.Request(
            url, data=body, method="POST",
            headers={
                "Content-Type": "application/json",
                # Cloudflare in front of hfrog returns "error code: 1010"
                # against any UA that looks like a script (e.g. urllib's
                # default Python-urllib/3.x). Identify ourselves clearly
                # so anyone reading WAF logs can trace it.
                "User-Agent": "hfrog_publisher/1.0 (+https://github.com/dwbmio/r_lit)",
                "Accept": "application/json",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                http = resp.status
                raw = resp.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as e:
            http = e.code
            raw = e.read().decode("utf-8", "replace")
        except urllib.error.URLError as e:
            err(f"{endpoint}  network error: {e.reason}")
            return False, str(e.reason)

        try:
            data = json.loads(raw)
        except json.JSONDecodeError:
            err(f"{endpoint}  http={http} non-JSON body: {raw[:200]}")
            return False, raw

        code = data.get("code")
        msg = str(data.get("msg") or "")
        if 200 <= http < 300 and code == 0:
            print(f"  ✓ {endpoint}", flush=True)
            return True, raw
        if DUPLICATE_PATTERNS.search(msg) or DUPLICATE_PATTERNS.search(raw):
            print(f"  = {endpoint}  (already exists)", flush=True)
            return True, raw
        print(f"::error::{endpoint}  http={http} code={code} msg={msg}", flush=True)
        return False, raw


# ─── R2 client (lazy import boto3) ─────────────────────────────────────────────

def _r2_client():
    try:
        import boto3
        from botocore.client import Config
    except ImportError:
        err("boto3 not installed. install with:  pip install --user boto3")
        sys.exit(2)
    for needed in ("R2_ENDPOINT", "R2_ACCESS_KEY_ID", "R2_SECRET_ACCESS_KEY", "R2_BUCKET"):
        if not os.environ.get(needed):
            err(f"missing env var: {needed}")
            sys.exit(2)
    return boto3.client(
        "s3",
        endpoint_url=os.environ["R2_ENDPOINT"],
        aws_access_key_id=os.environ["R2_ACCESS_KEY_ID"],
        aws_secret_access_key=os.environ["R2_SECRET_ACCESS_KEY"],
        region_name=os.environ.get("R2_REGION", "auto"),
        config=Config(signature_version="s3v4"),
    )


def r2_put(local_path: Path, key: str, *,
           content_type: str = "application/octet-stream",
           cache_control: str = "public, max-age=31536000, immutable") -> None:
    s3 = _r2_client()
    bucket = os.environ["R2_BUCKET"]
    s3.upload_file(
        str(local_path), bucket, key,
        ExtraArgs={"ContentType": content_type, "CacheControl": cache_control},
    )
    size = local_path.stat().st_size
    print(f"  ↑ s3://{bucket}/{key}  ({size} bytes)", flush=True)


# ─── helpers ──────────────────────────────────────────────────────────────────

def file_sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_assets(values: list[str]) -> list[tuple[str, Path]]:
    """Parse --asset target=path entries.  target is a Rust target triple."""
    out: list[tuple[str, Path]] = []
    for v in values:
        if "=" not in v:
            err(f"invalid --asset {v!r}, expected target=path")
            sys.exit(2)
        target, p = v.split("=", 1)
        path = Path(p).expanduser().resolve()
        if not path.is_file():
            err(f"--asset {target!r}: {path} not found")
            sys.exit(2)
        out.append((target, path))
    return out


def render_install_sh(template: Path, *, tool: str, version: str,
                       github_repo: str | None) -> str:
    text = template.read_text()
    return (text
            .replace("{{TOOL}}", tool)
            .replace("{{VERSION}}", version)
            .replace("{{GITHUB_REPO}}", github_repo or ""))


# ─── postgres patch (optional) ─────────────────────────────────────────────────

def patch_software_meta(pg_url: str, tool: str, *, fields: dict[str, Any],
                         category_code: str | None) -> None:
    try:
        import psycopg2
    except ImportError:
        warn("psycopg2 not installed. install:  pip install --user psycopg2-binary")
        return

    setters: list[str] = []
    params: list[Any] = []
    for col, val in fields.items():
        if val is None:
            continue
        setters.append(f"{col} = %s")
        params.append(val)

    if category_code:
        setters.append(
            "category_id = (SELECT id FROM rb_categories WHERE code = %s)"
        )
        params.append(category_code)

    if not setters:
        return

    setters.append("updated_at = CURRENT_TIMESTAMP")
    sql = (
        "UPDATE rb_softwares SET "
        + ", ".join(setters)
        + " WHERE name = %s"
    )
    params.append(tool)

    with psycopg2.connect(pg_url) as conn, conn.cursor() as cur:
        cur.execute(sql, params)
        if cur.rowcount == 0:
            print(f"  · {tool} not in hfrog yet (created on first /softwares POST)", flush=True)
        else:
            cat = f", cat={category_code}" if category_code else ""
            print(f"  ✎ {tool}  (SQL fields refreshed{cat})", flush=True)


# ─── core sync ─────────────────────────────────────────────────────────────────

def sync_hfrog_records(client: HfrogClient, *,
                        tool: str, version: str, description: str,
                        install_command: str, install_script_url: str,
                        source_type: str,
                        release_notes: str, created_by: str,
                        assets_with_checksum: list[tuple[str, Path, str, int, str]],
                        ) -> bool:
    """Push the four hfrog API records. Returns True on full success."""
    ok = True

    ok &= client.post("/api/release/softwares", {
        "name": tool,
        "description": description,
        "install_command": install_command,
        "install_script_url": install_script_url,
    })[0]

    seen_targets: set[str] = set()
    for target, _, _, _, _ in assets_with_checksum:
        if target in seen_targets:
            continue
        seen_targets.add(target)
        spec = DEFAULT_PLATFORM_DICT.get(target)
        if not spec:
            warn(f"unknown target {target!r}, sending generic platform record")
            os_, arch, display = "unknown", "unknown", target
        else:
            os_, arch, display = spec
        ok &= client.post("/api/release/platforms", {
            "code": target, "os": os_, "arch": arch, "display_name": display,
        })[0]

    ok &= client.post("/api/release/versions", {
        "software_name": tool,
        "version": version,
        "is_latest": True,
        "release_notes": release_notes,
        "created_by": created_by,
    })[0]

    for target, _, download_url, file_size, checksum in assets_with_checksum:
        ok &= client.post("/api/release/releases", {
            "software_name": tool,
            "version": version,
            "platform_code": target,
            "download_url": download_url,
            "file_size": file_size,
            "checksum_sha256": checksum,
            "source_type": source_type,
        })[0]

    return ok


# ─── subcommand: mirror ────────────────────────────────────────────────────────

def cmd_mirror(args) -> int:
    """Upload assets + (optional) install.sh to R2. No hfrog calls."""
    assets = parse_assets(args.asset)
    prefix = args.r2_prefix.rstrip("/")
    version_dir = f"{prefix}/v{args.version.lstrip('v')}"

    for target, path in assets:
        key = f"{version_dir}/{path.name}"
        r2_put(path, key)

    if args.sha256sums and args.sha256sums.is_file():
        r2_put(args.sha256sums, f"{version_dir}/SHA256SUMS",
               content_type="text/plain; charset=utf-8",
               cache_control="public, max-age=300")

    if args.install_template:
        text = render_install_sh(
            args.install_template, tool=args.tool, version=args.version.lstrip("v"),
            github_repo=args.github_repo)
        # latest pointer + version-pinned copy
        tmp_latest = Path(f"/tmp/_install_{args.tool}_latest.sh")
        tmp_versioned = Path(f"/tmp/_install_{args.tool}_v{args.version.lstrip('v')}.sh")
        tmp_latest.write_text(text); tmp_latest.chmod(0o755)
        tmp_versioned.write_text(text); tmp_versioned.chmod(0o755)
        r2_put(tmp_latest, f"{prefix}/install.sh",
               content_type="text/x-shellscript; charset=utf-8",
               cache_control="public, max-age=300")
        r2_put(tmp_versioned, f"{version_dir}/install.sh",
               content_type="text/x-shellscript; charset=utf-8",
               cache_control="public, max-age=31536000, immutable")
        tmp_latest.unlink(); tmp_versioned.unlink()
    return 0


# ─── subcommand: publish ───────────────────────────────────────────────────────

def cmd_publish(args) -> int:
    """End-to-end: optional R2 mirror + hfrog API + (optional) SQL patch."""
    assets = parse_assets(args.asset)
    if not assets:
        err("--asset required (at least one)")
        return 2

    version_no_v = args.version.lstrip("v")
    version_v    = "v" + version_no_v
    prefix       = args.r2_prefix.rstrip("/")
    version_dir  = f"{prefix}/v{version_no_v}"

    # 1. compute size + sha256 for every asset
    enriched: list[tuple[str, Path, str, int, str]] = []
    for target, path in assets:
        size = path.stat().st_size
        sha = file_sha256(path)
        if args.download_url_template:
            dl = (args.download_url_template
                  .replace("{tool}", args.tool)
                  .replace("{version}", version_v)
                  .replace("{version_no_v}", version_no_v)
                  .replace("{target}", target)
                  .replace("{filename}", path.name))
        else:
            dl = f"https://{args.r2_public_domain}/{version_dir}/{path.name}"
        enriched.append((target, path, dl, size, sha))
        info(f"  {target:36s}  {size:>12,d} bytes  sha256={sha[:16]}…  → {dl}")

    # 2. mirror to R2 (binaries + install.sh) — optional, but typically wanted
    if args.upload_r2:
        info("═══ R2 upload ═══")
        for target, path in assets:
            r2_put(path, f"{version_dir}/{path.name}")
        if args.sha256sums and args.sha256sums.is_file():
            r2_put(args.sha256sums, f"{version_dir}/SHA256SUMS",
                   content_type="text/plain; charset=utf-8",
                   cache_control="public, max-age=300")
        if args.install_template:
            text = render_install_sh(
                args.install_template, tool=args.tool, version=version_no_v,
                github_repo=args.github_repo)
            tmp_l = Path(f"/tmp/_install_{args.tool}_latest.sh")
            tmp_v = Path(f"/tmp/_install_{args.tool}_v{version_no_v}.sh")
            tmp_l.write_text(text); tmp_l.chmod(0o755)
            tmp_v.write_text(text); tmp_v.chmod(0o755)
            r2_put(tmp_l, f"{prefix}/install.sh",
                   content_type="text/x-shellscript; charset=utf-8",
                   cache_control="public, max-age=300")
            r2_put(tmp_v, f"{version_dir}/install.sh",
                   content_type="text/x-shellscript; charset=utf-8",
                   cache_control="public, max-age=31536000, immutable")
            tmp_l.unlink(); tmp_v.unlink()

    # 3. push hfrog records
    info("═══ HFrog sync ═══")
    client = HfrogClient(args.hfrog_api)
    ok = sync_hfrog_records(
        client,
        tool=args.tool, version=version_v, description=args.description,
        install_command=args.install_command,
        install_script_url=args.install_script_url,
        source_type=args.source_type,
        release_notes=args.release_notes or f"{args.tool} {version_v}",
        created_by=args.created_by,
        assets_with_checksum=enriched,
    )

    # 4. SQL patch (optional, e.g. category_id, llms_txt, readme_url)
    if args.postgres_url:
        info("═══ SQL patch (rb_softwares) ═══")
        fields: dict[str, Any] = {
            "description":        args.description,
            "install_command":    args.install_command,
            "install_script_url": args.install_script_url,
        }
        if args.readme_url:    fields["readme_url"] = args.readme_url
        if args.llms_txt_url:  fields["llms_txt"]   = args.llms_txt_url
        try:
            patch_software_meta(args.postgres_url, args.tool,
                                fields=fields, category_code=args.category)
        except Exception as e:
            warn(f"SQL patch failed: {e}")
            ok = False

    if not ok:
        err("publish completed with errors above")
        return 1
    info("✓ publish complete")
    return 0


# ─── subcommand: patch-meta ────────────────────────────────────────────────────

def cmd_patch_meta(args) -> int:
    """SQL-only refresh of rb_softwares row for one tool. No API, no R2."""
    if not args.postgres_url:
        err("--postgres-url required for patch-meta")
        return 2
    fields: dict[str, Any] = {}
    if args.description:        fields["description"]        = args.description
    if args.install_command:    fields["install_command"]    = args.install_command
    if args.install_script_url: fields["install_script_url"] = args.install_script_url
    if args.readme_url:         fields["readme_url"]         = args.readme_url
    if args.llms_txt_url:       fields["llms_txt"]           = args.llms_txt_url
    patch_software_meta(args.postgres_url, args.tool,
                        fields=fields, category_code=args.category)
    return 0


# ─── argparse ──────────────────────────────────────────────────────────────────

def _add_common_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("--tool",    required=True, help="software name (rb_softwares.name)")
    p.add_argument("--version", required=True, help="version, with or without leading v")
    p.add_argument("--asset",   action="append", default=[], metavar="TARGET=PATH",
                   help="asset to publish, repeat for each target. Example: "
                        "--asset x86_64-unknown-linux-gnu=./out/foo-linux.tar.gz")


def _add_metadata_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("--description", default="", help="rb_softwares.description")
    p.add_argument("--category",    default=None, help="rb_categories.code (cli/desktop/service)")
    p.add_argument("--source-type", default="open_source",
                   choices=["open_source", "internal"])
    p.add_argument("--install-command",    default="")
    p.add_argument("--install-script-url", default="")
    p.add_argument("--readme-url",         default="")
    p.add_argument("--llms-txt-url",       default="")


def _add_r2_args(p: argparse.ArgumentParser) -> None:
    p.add_argument("--r2-prefix", required=True,
                   help="R2 key prefix below bucket root, e.g. 'r_lit/bulk_upload' or 'hfrog'")
    p.add_argument("--r2-public-domain", default="r2.gamesci-lite.com",
                   help="public custom domain bound to the bucket")
    p.add_argument("--install-template", type=Path, default=None,
                   help="path to install.sh.template (with {{TOOL}} {{VERSION}} {{GITHUB_REPO}})")
    p.add_argument("--github-repo", default="",
                   help="owner/repo, used by install.sh fallback download URL")
    p.add_argument("--sha256sums", type=Path, default=None,
                   help="optional SHA256SUMS file to upload alongside")


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="hfrog_publisher",
        description="Single source of truth: publish artifacts to hfrog + R2.",
    )
    sub = ap.add_subparsers(dest="cmd", required=True)

    # publish (the workhorse)
    pp = sub.add_parser("publish", help="full sync: optional R2 mirror + hfrog API + SQL patch")
    _add_common_args(pp); _add_metadata_args(pp); _add_r2_args(pp)
    pp.add_argument("--hfrog-api", default=os.environ.get("HFROG_API",
                    "https://hfrog.gamesci-lite.com"))
    pp.add_argument("--upload-r2", action="store_true",
                    help="also mirror assets + install.sh to R2 (requires R2_* env)")
    pp.add_argument("--release-notes", default="", help="rb_versions.release_notes")
    pp.add_argument("--created-by", default="hfrog_publisher", help="rb_versions.created_by")
    pp.add_argument("--postgres-url", default=os.environ.get("HFROG_PG_URL"),
                    help="postgresql://user:pass@host:port/db, enables SQL patches")
    pp.add_argument("--download-url-template", default=None,
                    help="override download_url; placeholders: {tool} {version} {version_no_v} "
                         "{target} {filename}. Default: R2 public URL.")
    pp.set_defaults(func=cmd_publish)

    # mirror (R2 upload only — for stages that don't need hfrog)
    mp = sub.add_parser("mirror", help="upload assets and/or install.sh to R2 only")
    _add_common_args(mp); _add_r2_args(mp)
    mp.set_defaults(func=cmd_mirror)

    # patch-meta (SQL only)
    pm = sub.add_parser("patch-meta", help="UPDATE rb_softwares row via SQL only")
    pm.add_argument("--tool", required=True)
    _add_metadata_args(pm)
    pm.add_argument("--postgres-url", default=os.environ.get("HFROG_PG_URL"))
    pm.set_defaults(func=cmd_patch_meta)

    args = ap.parse_args(argv)
    return int(args.func(args) or 0)


if __name__ == "__main__":
    sys.exit(main())
