# mini_reg

A short-running Rust CLI for semi-automatic registration of company-owned WeChat,
Douyin, QQ and Alipay mini apps. One native executable contains the command line,
SQLite state database, HTTP/WebSocket browser transport and form logic. No Python,
Node.js, Playwright, external database or background service is required.

The interactive path requires an existing, dedicated `gamesci-lite.com` enterprise
Chrome profile with remote debugging enabled. The tool does not download, launch,
reconfigure or replace your browser. Personal or mixed profiles are refused.

## Current scope

- Import a TOML batch atomically; repeated imports preserve progress.
- Persist task state and the browser binding between runs.
- Open each platform's official portal in that explicitly bound browser.
- Preview or fill uniquely identified, visible name/summary text fields.
- Preserve existing different form values; refuse ambiguous fields.
- Save an interrupted write as `needs_review`, never as success.
- Record a user-confirmed AppID and export results without login data.

**Not yet acceptance-tested against live portal forms.** The current form adapter
uses exact labels or explicitly configured CSS selectors. Platform-specific
registration APIs for self-owned apps have not been verified and are not called.
Icon/document uploads, category choices, identity checks, agreements, payments
and final submission remain manual. App kind is mandatory in the manifest;
the operator must select the matching mini-program or mini-game creation form.

`confirmed` means a human checked and recorded the AppID. Exports always state
`platform_verified: false`; importing or filling never claims registration success.

## Distribution

The release path is the existing r_lit Jenkins worker pipeline, followed by R2
and HFrog. The tool is registered as `mini_reg`. A download URL is valid only
after a successful publication; this source change alone does not publish it.
SQLite is linked from its bundled source. HTTP uses rustls rather than OpenSSL.
Windows release builds omit PDB generation and retain static CRT linkage.
Standard OS libraries and the enterprise Chrome installation remain prerequisites.

## Quick start

```bash
mini_reg init batch.toml
# Edit company, task IDs, names, descriptions and app kinds in batch.toml.
mini_reg import batch.toml
mini_reg list --json
mini_reg next --json
```

Use `kind = "mini_program"` or `kind = "mini_game"` explicitly. The template
contains one task for each platform. A batch ID and task IDs are durable keys;
changing an existing task under the same ID is rejected, with no partial import.
Create a new task ID for a different application.

## Bind the enterprise browser

Use your existing dedicated enterprise Chrome process. It must have been started
with a non-default user-data directory and `--remote-debugging-port=0` by its owner.
Chrome writes `DevToolsActivePort` there. Do not expose the debugging port to LAN.
This tool deliberately supplies no command to start a default/personal browser.

```bash
mini_reg browser bind --user-data-dir /absolute/company-chrome-data --profile-directory Company
mini_reg browser tabs --json
```

Binding checks corporate account metadata, the single-profile inventory and the
active process's debugging fingerprint. Every browser operation checks again.
After a Chrome restart, bind again. Only account identity metadata is read;
cookies, passwords and authentication token stores are not read or exported.
If corporate identity cannot be verified, browser operations stop.

## Per-app handoff

`mini_reg next --open` resumes the first unconfirmed task in stable batch/task-ID
order. It opens a portal only if the task has no saved tab. Completed tasks are
skipped; interrupted tasks remain visible for review. Use `open TASK` explicitly
if a saved tab was closed. `next --batch ID` limits selection to one batch.

```bash
mini_reg open wechat-demo
# Log in and open the creation form for the manifest's app kind in that tab.
mini_reg fill wechat-demo
mini_reg fill wechat-demo --apply
# Check fields, choose categories and finish all platform confirmations yourself.
mini_reg confirm wechat-demo --app-id wx0123456789abcdef
mini_reg export results.json
```

`fill` without `--apply` is a dry run. It never changes form values or task phase.
`--tab ID` explicitly selects a portal tab returned by `browser tabs`; otherwise
the task's saved tab is used. No command clicks submit or accepts agreements.
Missing optional summary fields are reported as skipped. The name field is
required. Inputs inside cross-origin frames or closed shadow roots need manual
entry; the tool does not bypass them.

When a platform changes its form, persist exact selectors instead of patching
ad-hoc scripts. Selectors still must resolve to one visible, writable text field:

```bash
mini_reg selectors --platform wechat --name '#app-name' --summary '#app-summary'
# Omit both selectors to restore exact-label matching for that platform.
mini_reg selectors --platform wechat
```

## State, recovery and output

The default state path is the OS application-data directory for `mini_reg`.
Pass `--db /absolute/state.sqlite3` on any command to choose another location.
SQLite transactions, WAL and FULL synchronization protect committed progress.
Each app uses `planned -> opened -> needs_review -> waiting_human -> confirmed`.
The `needs_review` checkpoint is committed before a browser write; after an error
or crash, inspect the page before explicitly running `fill --apply` again.
No final registration request is automatically retried.

`--json` produces one stdout envelope. Logs go to stderr and a replaced per-run
sidecar: `<database-stem>.log`, or `<output-stem>.log` for init/export. CLI arguments
and browser payloads are not logged. A failed command exits with status 1 and
still flushes its error log. Manifest/export files refuse overwrite. Treat the
database and exports as internal company metadata. Do not place secrets in them.

Backup or move a database with SQLite's backup API, or after all mini_reg processes
have exited and its WAL has been checkpointed. Do not copy only a live `.sqlite3`
file while it has a separate WAL.

## Development

Build and test only on dedicated CI workers, never on the Jenkins controller or
production/limited machines. The independent `[workspace]` isolates this crate
from the root's current desktop workspace. On a worker:

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

Tests cover process restarts, idempotency, rollback, interrupted writes, conflicting
AppIDs, secret-field rejection, official-origin checks, enterprise-profile checks
and the CLI JSON/export path. They do not replace real-channel registration
acceptance. See [architecture](docs/ARCHITECTURE.md).
