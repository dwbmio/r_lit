# mailctl

Multi-mailbox IMAP/SMTP dispatcher: **one config directory, send and receive per mailbox identity** with allowlist, confirm-before-send, and audit logging.
Pure-Rust single binary (`rustls` + `webpki-roots`, no OpenSSL, no runtime interpreter dependencies).

## Capability Manifest (Shared for Humans and AI)

The machine-readable version is emitted directly by the binary from the embedded `capabilities.json`:

```bash
mailctl capabilities --json | jq .
```

| Command | Purpose | Network | Side Effects |
|---|---|---|---|
| `accounts` | List configured accounts (endpoints / credential source / source file / signature first line); `--json` | No | None |
| `folders` | List mailbox folders (IMAP modified-UTF-7 decoded) | IMAP | None |
| `verify` | Test IMAP + SMTP login without sending mail | IMAP+SMTP | None |
| `scan` | Read-only header scan (`--limit` `--unseen` `--folder` `--criteria` `--match` `--json`) | IMAP | None (`EXAMINE` + `BODY.PEEK`) |
| `read` | Read message body (`GBK`/`GB18030`/`UTF-8` + nested multipart); `--raw` prints raw RFC822 | IMAP | None |
| `draft` | Create `.eml` draft with per-account signature, without sending | No | Writes `.eml` |
| `reply` | Generate threaded reply draft by UID (inherits `In-Reply-To` / `References`), without sending | IMAP | Writes `.eml` |
| `send` | Send `.eml`: requires `--yes`, enforces allowlist and sender identity; `--append-sent` saves to Sent | SMTP (+IMAP) | **External email + audit line** |
| `selftest` | Loopback test: send to self and read back from INBOX (never contacts third parties) | IMAP+SMTP | Sends to self + audit |
| `capabilities` | Emit capability manifest JSON | No | None |

## Four Guard Rails (Non-Bypassable)

1. **Credentials**: Keychain (`keychain_service`) > env var > masked interactive input; passwords are never stored in config files.
2. **Allowlist**: `allow_recipients` is scoped per account; mismatches are rejected (exit 1) with **no CLI bypass**; self-address is always allowed.
3. **Confirm before send**: When `confirm_before_send = true` (default), `send` requires `--yes` or exits with code 3.
4. **Sender identity**: `.eml` `From` header must match the selected account address.

Additional safeguards: plaintext SMTP/IMAP is restricted to `127.0.0.1`/`localhost`; `scan` and `read` are strictly read-only (never mark seen, never delete).

## Configuration: One File per Mailbox

```
~/.config/hermes-mail/accounts/
├── qq-personal.toml          # active, account name = filename stem
└── work-exmail.toml.example  # template, ignored
```

```toml
[account]
provider = "qq"                      # qq / exmail / aliyun-enterprise / aliyun / gmail / outlook / custom
address  = "user@qq.com"
display_name = "Alice | Example Org"
auth.keychain_service = "mail-qq-personal"
sent_folder = "Sent Messages"
allow_recipients = ["apple.com", "*.apple.com"]
signature = """
Alice
Head of Operations
Example Org
"""
```

You can also use `signature_file = "sig.txt"` (resolved relative to the account TOML file). Signatures are isolated per account; pass `--no-signature` to omit temporarily.

## Common Workflows

```bash
# Initial setup
security add-generic-password -U -a user@qq.com -s mail-qq-personal -w '<app-password>'
mailctl verify   -a qq-personal
mailctl selftest -a qq-personal --wait 90      # self-send loopback verification

# Reply to a message
mailctl scan  -a qq-personal --match 'apple' --json
mailctl read  -a qq-personal --uid 3016
mailctl reply -a qq-personal --uid 3016 --body-file body.txt --out /tmp/r.eml
mailctl send  -a qq-personal --eml /tmp/r.eml --yes --append-sent
mailctl scan  -a qq-personal --folder 'Sent Messages' --limit 5
```

## Exit Codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Runtime error (auth / allowlist blocked / sender mismatch) |
| 2 | Usage error (clap / invalid flags) |
| 3 | Input error (refused to send without `--yes`, missing file) |
| 4 | External service error (IMAP / SMTP / TLS failure) |

Also supports global `--json` (standard `rlit_cli` envelope) and `mailctl __schema`.

## Build & Release

```bash
cargo test            # unit + E2E tests (built-in mock IMAP/SMTP, offline)
cargo build --release
```

Published via the internal Jenkins `r_lit` pipeline (`ci-all-in-one/task/ci/pipeline/r_lit/`).

## Known Boundaries

- Receiving uses IMAP (not POP3): POP3 has no folders, no stable UID addressing, and cannot read back Sent.
- Provider quirks: QQ Mail rewrites `Message-ID` on delivery (threading relies on `References`); some SMTP servers do not auto-save to Sent, requiring `--append-sent`.
- Not implemented: attachments, HTML body rendering, IMAP IDLE (poll with `scan --unseen`).
