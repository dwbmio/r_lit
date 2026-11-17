# phone_haul

Phone media migration tool with dual Android/iOS stacks:
- **Android**: static `aarch64-linux-android` agent pushed via `adb` + Mac host orchestrator (`scan` / `pull`, parallel `exec-out` streams, BLAKE3 dedup, `yyyy/MM` archive).
- **iOS**: direct **AFC** protocol over pure-Rust `idevice` + `/PhotoData/Photos.sqlite` diff (no Python dependencies).

## iOS Diff Workflow (Pure Rust)

```bash
phone_haul ios-scan --out DIR                          # pull Photos.sqlite over AFC -> iphone_keys.json
phone_haul diff --archive ~/Downloads/xiaomi_backup --out ~/Downloads/to_iphone
phone_haul push-ios --from ~/Downloads/to_iphone --dest ~/Downloads/iphone_sync
phone_haul collect-lowconf --archive ~/Downloads/xiaomi_backup --out ~/Downloads/review_screenshots
```

Identity key = absolute unix seconds (capture time) + width + height. JPEG reads EXIF `DateTimeOriginal` (converted to UTC), MP4/MOV reads `mvhd` `creation_time`, and iPhone reads `ZASSET.ZDATECREATED` (CoreFoundation epoch) via `rusqlite`. Missing items are hardlinked to `--out` for Finder iPhone photo sync.

Known boundary: PNG/HEIC files without cheap keys are grouped as `low_confidence` and excluded from automatic matching; Camera Roll AFC is read-only.

## Architecture

```
Android phone_haul_agent (aarch64-linux-android, static)
  scan   : walk media dirs -> magic classification -> quick hash -> manifest JSON (stdout)
  stream : read path list from stdin -> frame stream ("ADH1" LE: [path_len u32][path][size u64][payload]...)

Mac host phone_haul
  scan : adb push agent -> exec-out scan -> manifest.json + summary
  pull : redb dedup -> N parallel `adb exec-out` streams -> streaming BLAKE3 verify -> yyyy/MM archive -> redb update
```

## Key Techniques

| Technique | Location | Notes |
|---|---|---|
| Magic-byte classification | `classify()` reads first 512B | JPEG/PNG/GIF/WEBP/HEIC/AVIF/MP4/MOV/WEBM, ignores file extension |
| Two-tier BLAKE3 dedup | tier1 quick hash (head/mid/tail 64KiB + size) + tier2 full streaming BLAKE3 | `redb`: `b3 -> path` |
| Time-ordered archive | `yyyy/MM` folders | Civil-from-days calculation, zero `chrono` runtime cost on agent |

## Build

```bash
# host (macOS)
cargo build --release --bin phone_haul

# agent (Android cross-compile via zigbuild / cargo-ndk)
cargo zigbuild --release --target aarch64-linux-android --bin phone_haul_agent
mkdir -p out/aarch64-linux-android
cp target/aarch64-linux-android/release/phone_haul_agent out/aarch64-linux-android/
```

## Usage

```bash
# 1. Scan Android device (device must be unlocked so FBE storage is readable)
phone_haul scan --out ~/Downloads/xiaomi_backup

# 2. Pull with deduplication (re-running acts as incremental backup)
phone_haul pull --dest ~/Downloads/xiaomi_backup -j 8

# Images only
phone_haul pull --dest ~/Downloads/xiaomi_backup --images-only
```
