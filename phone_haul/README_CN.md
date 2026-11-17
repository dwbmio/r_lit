# phone_haul (原 adb_haul)

手机照片/视频迁移工具，支持 Android 与 iOS 双栈：
- **Android**：`adb` 推送设备端静态 `phone_haul_agent` + Mac 端 `phone_haul` 编排（并行流 + 魔数判型 + 两级 BLAKE3 排重 + `yyyy/MM` 归档）。
- **iOS**：纯 Rust `idevice` 直连 **AFC 协议**读取 `/PhotoData/Photos.sqlite` 做差量比对（无 Python 依赖）。

## iOS 差异比对（纯 Rust）

```bash
phone_haul ios-scan --out DIR                          # AFC 拉取 Photos.sqlite -> iphone_keys.json
phone_haul diff --archive ~/Downloads/xiaomi_backup --out ~/Downloads/to_iphone
phone_haul push-ios --from ~/Downloads/to_iphone --dest ~/Downloads/iphone_sync
phone_haul collect-lowconf --archive ~/Downloads/xiaomi_backup --out ~/Downloads/review_screenshots
```

身份键 = 拍摄绝对 Unix 秒 + 宽 + 高（JPEG 读 EXIF `DateTimeOriginal`，MP4/MOV 读 `mvhd` `creation_time`，iPhone 读 `ZASSET.ZDATECREATED`）。未命中文件硬链到 `--out`，供 Finder 同步到 iPhone。

已知边界：无廉价时间键的 PNG/HEIC 归为 `low_confidence`，不参与自动比对；Camera Roll AFC 为只读。

## 实测记录 (2026-05, Xiaomi MIX Flip / Apple Silicon)

| 环节 | 数值 | 标签 |
|---|---|---|
| 手机 `/sdcard` FUSE 顺序读 | 673 MB/s ~ 1.0 GB/s | `[MEASURED]` |
| `adb` 8 路 pull 全量实迁 | ~45 MB/s (17.22 GiB / ~6.5 min) | `[MEASURED]` |
| USB 协商速率 | 480 Mbps (USB 2.0 硬件上限) | `[MEASURED]` |
| iPhone AFC 拉取 437MB `Photos.sqlite` | ~35 MB/s，diff 全程 13s | `[MEASURED]` |

## 构建与使用

```bash
# host (macOS)
cargo build --release --bin phone_haul

# agent (Android 交叉编译)
cargo zigbuild --release --target aarch64-linux-android --bin phone_haul_agent

# 1. 扫描（需手机已解锁）
phone_haul scan --out ~/Downloads/xiaomi_backup

# 2. 迁移（重复执行即增量备份）
phone_haul pull --dest ~/Downloads/xiaomi_backup -j 8
```
