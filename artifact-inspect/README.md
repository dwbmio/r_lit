# artifact-inspect

`artifact-inspect` is a local Rust CLI for repeatable, offline inventories of game build outputs. It accepts a directory, ZIP, TAR.GZ, or TGZ and writes a versioned JSON report, a standalone HTML report, and one run log. Nested ZIP files are discovered by filename or signature at any path, including `out/engine/godot.zip`; no C4-specific path is hard-coded.

Canonical TAR root directory entries named exactly `.` or `./` are accepted but do not become inventory rows. Other directory paths are still validated; empty normalized non-directory paths remain input errors. No root marker is required, and ZIP path handling is unchanged.

The tool lists files only. It does not run game code, execute archived scripts, extract archives into the input, access the network, or claim Godot/Unity asset provenance.

## Build

Build this independent crate from its own directory, not from the repository root:

```bash
cd artifact-inspect
cargo build --release --locked
```

Requires Rust with edition 2024 support and Cargo. The committed [Cargo.toml](Cargo.toml) declares version 0.1.0 and the independent [Cargo.lock](Cargo.lock) pins dependencies; the root workspace excludes this crate. Build only on an approved, adequately provisioned development/CI worker, never on a Jenkins controller, production/service host, or a host with at most 2 CPU cores or 4 GiB RAM.

The Cargo binary is `target/release/artifact-inspect`; invoke it by that path or put its directory on `PATH` for the examples below. Retained local binaries use `out/<target-triple>/artifact-inspect`; `out/` is ignored by Git. Validation covers local macOS arm64 and the Linux x86_64 Jenkins pilot `arcrate/package-analysis-pilot #1`. The pilot binary is not a registered release or installer. Windows has not been validated.

The root `.justfile` generically routes `just build artifact-inspect release` and `just install_loc artifact-inspect release` to Cargo (without the explicit `--locked` flag); its fixed `test-cli`/`lint-cli` lists do not include this tool. These routes are not release registration. Do not use an assumed R2 installer URL.

## Usage

```bash
artifact-inspect dist/ --output-dir inspection/
artifact-inspect game.zip -o inspection/ --json
artifact-inspect package.tar.gz -o inspection/ --max-depth 4
artifact-inspect __schema
```

The output directory must not be the input directory or one of its descendants. Existing JSON/HTML/log destinations must be ordinary files: symbolic links, hard links (on Unix), non-regular files, and any destination that is the input file itself are rejected with exit 2 before inspection. Ordinary reports are atomically replaced on a repeated run; destination links are never followed and input files are never modified.

### Output files

| File | Purpose |
|---|---|
| `artifact-inspect.json` | Full report with `schema_version: 1`, input, effective limits, containers, files, previews, issues, and limitations |
| `artifact-inspect.html` | Offline per-row browser view; no JavaScript, external resources, or active archived HTML |
| `artifact-inspect.log` | One `rlit_cli::runlog` sidecar for the current run, including failures after output validation |

With `--json`, stdout is the standard [`rlit_cli` envelope](../docs/cli-contract.md) (`contract: 1`, runtime `command: ""`). Its `data` contains container summaries and report paths; `artifacts` contains the JSON, HTML, and log with sizes and SHA-256 values. The complete inventory is hierarchical at `containers[].files[]`, not a flat top-level file list and not duplicated into stdout. `__schema` exports CLI parameters, not the report schema; its sole leaf has `name: "artifact-inspect"`, `argv_prefix: []`, required `input`/`output_dir`, and `x-effects: "pure"` (declared output writes, not zero filesystem writes).

HTML provides per-layer metadata, bounded escaped text and supported image previews, not an interactive engine viewer. Output validation failures and argument parsing errors do not produce a sidecar log; later failures attempt to flush `artifact-inspect.log`. In JSON mode human logs go to stderr. Read that log first when debugging. Reports are replaced individually, not as a three-file transaction. A failed rerun does not remove old JSON/HTML; use a fresh output directory or check the current exit/envelope and artifact receipts, never treat stale reports as current success. Log timestamps and absolute input/output paths are not portable determinism guarantees. Per-container file rows use lexicographic relative-path ordering, not descending size; pending ZIP children are sorted within each parent and queued breadth-first, not globally sorted across parents.

### File and size semantics

Each file row contains:

- `container_id` and `container`: the outer input or logical nested container path (`parent.zip!nested.zip`).
- `relative_path`: a normalized archive member or directory-relative path. Rows are sorted lexicographically within each container; nested containers are processed in sorted breadth-first order.
- `kind`: detected bitmap, text, archive, binary, Godot, Unity, or other type.
- `raw_bytes`: uncompressed file size.
- `zip_compressed_bytes`: the ZIP central-directory compressed entry payload size. It is `null` for directory files and TAR.GZ members because TAR.GZ has no reliable per-member compressed-byte value.
- `preview`: preview kind, limitation, truncation state, and optional bounded text or bitmap data.

Every container has independent `file_count`, `raw_bytes`, and (for ZIP only) `zip_compressed_bytes` totals. There is deliberately no cross-layer byte sum: a nested ZIP is already a file in its parent, so adding its members to the parent total would double-count storage. ZIP entry payload sizes exclude ZIP headers and central-directory overhead and are not network transfer measurements.

Godot packages/resources and Unity scenes/assets are classified but not parsed. The report does not infer engine asset provenance, import dependencies, or actual runtime transfer volume.

## Preview safety and limits

- PNG, JPEG, GIF, and WebP previews are dimension-checked, decode-validated, pixel-limited, and embedded as inert `data:` URIs.
- UTF-8 text is byte-bounded and HTML-escaped. HTML, JavaScript, and SVG are shown only as escaped text, never activated.
- Sensitive-looking files such as private/secret/credential/token configuration, any `*.env`, `.env.*`, `.envrc`, private keys, and keystores retain path and byte rows but have `preview.kind = "suppressed_sensitive"` and no text or image content. Members of a sensitive nested ZIP inherit that suppression while retaining their own paths and sizes.
- The HTML CSP is `default-src 'none'`; only `data:` images and inline report styling are allowed.
- Input symlinks and archive links are not followed. Special files are skipped with an issue. Unsafe archive paths are rejected.
- Nested ZIPs are held in bounded memory, never extracted beside the input. Every regular ZIP member is streamed to its end with a fixed-size buffer to validate decompression, declared size, and CRC beyond the preview prefix. A corrupt nested ZIP is recorded as `nested_zip_invalid`; a corrupt outer ZIP exits with an input error instead of producing a success report.
- After TAR members are read, the gzip decoder is drained with a fixed-size buffer through the gzip trailer so truncation and checksum failures cannot be hidden by an early TAR end marker. A corrupt outer TAR.GZ exits 3 with `error.kind = "input"`, writes no JSON/HTML success report, and retains an error log.

Default limits:

| Option | Default | Meaning |
|---|---:|---|
| `--max-entries` | 100000 | Regular files processed across all layers |
| `--max-total-raw-bytes` | 2000000000 | Cross-layer processing workload bound, not a report total |
| `--max-nested-bytes` | 67108864 | Maximum bytes of one nested ZIP file before expansion |
| `--max-depth` | 4 | Nested ZIP depth; 0 disables expansion |
| `--max-preview-bytes` | 65536 | Maximum source bytes embedded per preview |
| `--max-total-preview-bytes` | 8388608 | Total source bytes embedded in previews |
| `--max-image-pixels` | 16777216 | Bitmap preview pixel limit |

Limit exceedances and skipped content are explicit errors or per-container issues; no substitute data is generated. `max_entries` also bounds directory walk entries (including the root and directories) and the outer/each nested ZIP central-directory entry count (including directories). The regular-file counter and raw-byte workload are shared across layers. `max_nested_bytes` limits the stored nested ZIP file bytes (before expanding its members), not the sum of its members. Entry, total-raw-byte, total-preview-byte and image-pixel limits must be positive; depth, nested-byte and per-preview-byte limits may be zero. Schema integer minimums are zero; these extra runtime checks still apply.

Only ZIPs are expanded recursively: nested TAR/GZIP/Brotli and proprietary engine containers are not unpacked. ZIP support is limited by the committed `zip` dependency features (`deflate`, default features disabled); encrypted or unsupported compression formats are not promised.

Sensitive suppression is a path/name heuristic, not a content secret scan or a confidentiality guarantee. Paths, sizes, and unsuppressed text/images remain in reports. Review reports before sharing; the tool does not prove them safe for external publication. Run on a stable local input and use a controlled output directory, not an adversarial concurrently modified filesystem.

## Exit codes

- `0`: success; non-fatal skips, if any, are listed in the report and envelope warnings.
- `1`: remaining runtime, serialization, or I/O errors (not every output-write error is runtime).
- `2`: invalid arguments/limits/output location, input symlinks, or rejected output targets.
- `3`: missing, unreadable, unsupported, corrupt, unsafe, or limit-exceeding outer input. I/O `NotFound`, `PermissionDenied`, `InvalidData`, `InvalidInput`, and `UnexpectedEof` also map here, even when raised during output/log writes.
- `4` / `5`: reserved by the shared contract; this offline single-input tool does not normally emit them.

## Tests

```bash
cd artifact-inspect
cargo test
cargo clippy --all-targets -- -D warnings
```

The isolated test suite covers directory, ZIP and TAR.GZ inputs; nested ZIP discovery by signature; stable per-layer sizes; truncated gzip trailers; ZIP CRC corruption beyond previews; explicit nested-ZIP failures; symlinks and hard-linked output targets; ordinary output replacement; output placement; limits; offline/escaped previews; environment-file and sensitive-container suppression; the CLI envelope; and `__schema`.

For the frozen local Zuma C4 regression, replace both placeholder paths with existing operator-supplied inputs and run from `artifact-inspect/`. No C4 fixture or private inventory is bundled. The following is an opt-in comparison against the Lead's independently generated inventory, not part of ordinary CLI use:

```bash
C4_INPUT="/path/to/frozen-c4.tar.gz" \
C4_REFERENCE="/path/to/independent-c4-inventory.json" \
cargo test --test c4_reference -- --ignored --nocapture
```

The historical local macOS arm64 regression on 2026-11-20 used these baselines; this is not a fresh cross-platform measurement. That test compares every `(layer, path)` with raw and ZIP compressed bytes and checks the independent totals: 25 outer files / 7,696,698 B and 139 nested files / 2,093,130 B raw / 1,979,382 B ZIP entry payload. It also verifies that `out/project.private.config.json` and `env/runtime.env` are both `suppressed_sensitive` with no text, media type, or image content; only those metadata states are printed, never their values.
