# phone_haul (原 adb_haul)

Phone media migration, dual stack:
- **Android**: on-device agent (musl static binary via adb push) + host orchestrator
- **iOS**: stock iOS 无法跑设备端二进制 → host 端直说 **AFC 协议**；元数据走 `/PhotoData/Photos.sqlite`（照片库权威 DB，一次拉取免去逐文件 hash），比对/排重核心仍在 Rust/Python 胶水只做采集

## iOS 差异比对 (纯 Rust, 无 python 依赖)

```bash
phone_haul ios-scan --out DIR                          # AFC 拉 Photos.sqlite → iphone_keys.json
phone_haul diff --archive ~/Downloads/xiaomi_backup --out ~/Downloads/to_iphone
phone_haul push-ios --from ~/Downloads/to_iphone --dest ~/Downloads/iphone_sync   # 重组为 yyyy/MM 相册树供 Finder 同步
phone_haul collect-lowconf --archive ~/Downloads/xiaomi_backup --out ~/Downloads/review_screenshots  # 无键文件(截图等)独立归集
```

身份键 = 绝对 unix 秒（拍摄时间） + 宽 + 高。JPEG 读 EXIF DateTimeOriginal（本地墙钟，减 utc offset 转绝对值），
MP4/MOV 读 mvhd creation_time（本身即 UTC）；iPhone 侧 rusqlite 读 ZASSET.ZDATECREATED（CF epoch）。
未命中的硬链到 --out，供 Finder → iPhone → 照片同步导入。

实测 (iPhone 17 系列 / iOS 26.6.1): AFC 拉 437MB Photos.sqlite ~35 MB/s，diff 全程 13s [MEASURED]。

已知边界：PNG/HEIC 无廉价键 → low_confidence 不参与比对；Camera Roll AFC 只读，
写入只能走 Finder 照片同步或 App 文档共享目录。

Parallel, dedup-aware photo/video migration from Android devices to local disk over adb.
双端 binary：手机端 agent 干完即退，Mac 端 host 负责编排、排重、校验、按时间归档。

> 性能数据见文末「实测记录」。

## 实测记录 (2026-05, Xiaomi MIX Flip 2405CPX3DC / MacBook Apple Silicon)

| 环节 | 数值 | 标签 |
|------|------|------|
| 手机 /sdcard FUSE 顺序读 | 673 MB/s ~ 1.0 GB/s | [MEASURED] |
| adb 单路 exec-out | 16.8 MB/s | [MEASURED] |
| adb 4 路并行聚合计 | 31.9 MB/s (扩展性仅 ~2x) | [MEASURED] |
| **adb 8 路 pull 全量实迁** | **~45 MB/s (17.22 GiB / ~6.5 min)** | [MEASURED] |
| USB 协商速率 | 480 Mbps (USB 2.0, 手机端口硬件上限) | [MEASURED] ioreg DeviceSpeed=2 |
| **瓶颈结论** | 并行已打满 USB 2.0 物理链路 (~80% 利用率)，再调系统 IO 无意义 | [MEASURED] |
| zstd -3 压 JPEG 样本 | 21.9% 缩减 (104.86MB → 81.91MB) | [MEASURED] 本数据集 |
| zstd -3 压 MP4 样本 | 1.6% 缩减 | [MEASURED] |
| 混合压缩收益 (11.6G JPEG + 5.8G MP4) | ~15% 时间缩减 — 小杠杆 | [MEASURED] 推算 |
| **WiFi LAN TCP 直连 (绕开 USB)** | 预计 5-10x — **唯一颠覆性杠杆** | [HYPOTHETICAL] 待实现后实测 |

结论：链路层是瓶颈。系统 IO 优化 (splice/mmap/F_NOCACHE) 在 link-bound 下零收益，不投入 (R3)。
优先级: 1. WiFi TCP transport; 2. zstd 可选 flag (link-bound 下 ~15%); 3. 其余不投入。

## 架构

```
手机端 phone_haul_agent (aarch64-linux-android, 静态)
  scan   : 遍历媒体目录 → 魔数判型 → quick hash → manifest JSON (stdout)
  stream : stdin 收路径清单 → 帧协议流式输出文件内容
      帧格式 (LE): magic u32 "ADH1"
                   [path_len u32][path][size u64][payload]... path_len=0 结束

Mac 端 phone_haul
  scan : adb push agent → exec-out scan → manifest.json + 汇总报表
  pull : redb 排重 → 按批分给 N 路并行 `adb exec-out` → 边收边算 BLAKE3
         → 校验 → yyyy/MM/ 落盘 → 更新 redb (增量备份)
```

为什么不用 `adb pull`：单连接、无校验、无排重、无内容判型。
N 路并行 exec-out 把协议调度开销摊薄，瓶颈收敛到 adb/USB 通道本身 [HYPOTHETICAL]。

## 关键手段

| 手段 | 落点 | 备注 |
|------|------|------|
| 魔数判型 | `classify()` 读前 512B | JPEG/PNG/GIF/WEBP/HEIC/AVIF/MP4/MOV/WEBM，不认扩展名 |
| 两级排重 | tier1 quick = blake3(头/中/尾 64KiB + size)，设备端预筛同机重复 (DCIM vs 相册云缓存只传一份)；tier2 全量 BLAKE3 边收边算，零额外 IO，权威排重 | redb: b3 → path |
| SIMD | blake3 crate 自带 NEON (Apple Silicon + 骁龙) | 哈希吞吐 [HYPOTHETICAL] GB/s 级，远低于 IO 瓶颈，不手写 SIMD |
| 多线程 | host N 路 adb 流 (默认 8)；agent scan 阶段 rayon 并行 (roadmap) | |
| 系统 API | `ftruncate` 预分配 + 顺序写；roadmap: `fcntl(F_NOCACHE)` 绕 page cache、分批 `pwritev` | macOS 无 io_uring |
| 时间归档 | v1 按 mtime → yyyy/MM；roadmap: EXIF DateTimeOriginal 优先 | 无 chrono 依赖，civil-from-days 手算 |

## 构建

```bash
# host (本机 macOS)
cargo build --release --bin phone_haul

# agent (交叉编译，建议 zigbuild / cargo-ndk)
cargo zigbuild --release --target aarch64-linux-android --bin phone_haul_agent
mkdir -p out/aarch64-linux-android
cp target/aarch64-linux-android/release/phone_haul_agent out/aarch64-linux-android/
```

## 使用

```bash
# 1. 扫描 (需手机已解锁, FBE 解密后 /sdcard 才可读)
phone_haul scan --out ~/Downloads/xiaomi_backup

# 2. 迁移 (含排重报表; 重复执行即增量备份)
phone_haul pull --dest ~/Downloads/xiaomi_backup -j 8

# 只迁图片
phone_haul pull --dest ~/Downloads/xiaomi_backup --images-only
```

## Roadmap

- [x] iOS native Rust AFC (idevice crate) + Photos.sqlite diff — 已替代 python 胶水
- [ ] iOS low-confidence 文件（PNG/HEIC）二次比对：按 size 候选 + AFC 范围读头部哈希
- [ ] Android agent scan rayon 并行哈希
- [ ] EXIF DateTimeOriginal 归档键 (kamadak-exif，仅 JPEG/HEIC/TIFF 头解析)
- [ ] 断点续传：staging 已收字节对账 manifest
- [ ] WiFi LAN 传输通道 (agent 起 TCP server，绕开 adb/USB 瓶颈)
- [ ] bench 对照：adb pull vs 本工具 N∈{1,4,8,16}，转 [MEASURED]
- [ ] F_NOCACHE / pwritev 落盘优化
- [ ] 小米相册云缓存目录 (.thumbnails / gallery cloud cache) 默认排除规则
