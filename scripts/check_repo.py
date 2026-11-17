#!/usr/bin/env python3
"""Repository hygiene, documentation, and Cargo isolation validator for r_lit."""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

TOOLS_AND_LIBS = [
    "audiogen",
    "bulk_upload",
    "deskpet",
    "gamereel",
    "group_vibe_workbench",
    "img_resize",
    "looplog",
    "mailctl",
    "maquette",
    "mini_reg",
    "mj_atlas",
    "mpreview",
    "phone_haul",
    "textexture",
    "ui-trim",
    "crates/murmur",
    "crates/rlit_cli",
]

REQUIRED_DOCS = ("README.md", "README_CN.md", "llms.txt", "llms_cn.txt")
CJK_RE = re.compile(r"[\u4e00-\u9fff]")
STALE_PATTERNS = (
    re.compile(r"snappywatt\.com", re.IGNORECASE),
    re.compile(r"nexus\.gamesci-lite\.com", re.IGNORECASE),
    re.compile(r"github\.com/dwbmio/r_lit/releases", re.IGNORECASE),
    re.compile(r"github\.com/dwbmio/r_lit/workflows/Release", re.IGNORECASE),
    re.compile(r"github\.com/\.\.\."),
)

# Historical audit/migration docs that intentionally mention deprecated URLs.
STALE_EXEMPT = {
    "docs/release.md",
}


def check_docs(errors: list[str]) -> None:
    targets = ["."] + TOOLS_AND_LIBS
    for rel in targets:
        d = ROOT / rel
        if not d.is_dir():
            errors.append(f"missing expected directory: {rel}")
            continue
        for doc in REQUIRED_DOCS:
            p = d / doc
            if not p.is_file():
                errors.append(f"missing required doc: {rel}/{doc}")
                continue
            text = p.read_text(encoding="utf-8", errors="replace")
            if doc in ("README.md", "llms.txt") and CJK_RE.search(text):
                errors.append(f"CJK characters found in English doc: {rel}/{doc}")
            if doc in ("README_CN.md", "llms_cn.txt") and not CJK_RE.search(text):
                errors.append(f"no Chinese characters found in Chinese doc: {rel}/{doc}")
            norm = f"{doc}" if rel == "." else f"{rel}/{doc}"
            if norm not in STALE_EXEMPT:
                for pat in STALE_PATTERNS:
                    if pat.search(text):
                        errors.append(f"stale URL ({pat.pattern}) in {norm}")


def check_cargo_excludes(errors: list[str]) -> None:
    root_toml = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    for p in sorted(ROOT.glob("*/Cargo.toml")) + sorted(ROOT.glob("crates/*/Cargo.toml")):
        rel_dir = str(p.parent.relative_to(ROOT))
        if rel_dir == "deskpet":
            continue
        if f'"{rel_dir}"' not in root_toml:
            errors.append(f"crate {rel_dir} is missing from root Cargo.toml workspace.exclude")


def check_no_duplicate_runlog(errors: list[str]) -> None:
    for crate in ("mj_atlas", "bulk_upload", "img_resize", "textexture", "mini_reg"):
        dup = ROOT / crate / "src" / "runlog.rs"
        if dup.exists():
            errors.append(f"duplicate {crate}/src/runlog.rs should use rlit_cli::runlog")


def check_no_broken_symlinks(errors: list[str]) -> None:
    for cur, dirs, files in os.walk(ROOT):
        if "/.git" in cur or "/target" in cur:
            dirs[:] = []
            continue
        for name in dirs + files:
            fp = Path(cur) / name
            if fp.is_symlink() and not fp.exists():
                errors.append(f"broken symlink: {fp.relative_to(ROOT)}")


def main() -> int:
    errors: list[str] = []
    check_docs(errors)
    check_cargo_excludes(errors)
    check_no_duplicate_runlog(errors)
    check_no_broken_symlinks(errors)

    if errors:
        print("check_repo.py FAILED:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print(f"check_repo.py OK ({len(TOOLS_AND_LIBS)} subprojects + root verified)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
