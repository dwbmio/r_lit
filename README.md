# R_LIT 工具集

跨平台 CLI 工具集，用于图片处理和文件上传。

[![CI](https://github.com/dwbmio/r_lit/workflows/CI/badge.svg)](https://github.com/dwbmio/r_lit/actions)
[![Release](https://github.com/dwbmio/r_lit/workflows/Release/badge.svg)](https://github.com/dwbmio/r_lit/actions)

## 工具列表

- **bulk_upload** - 批量下载 URL 并上传到 S3 对象存储
- **img_resize** - 图片尺寸调整和压缩工具

## 快速安装

### 一键安装（推荐）

```bash
curl -fsSL https://raw.githubusercontent.com/dwbmio/r_lit/main/install.sh | sh
```

### 使用 Cargo

```bash
cargo install bulk_upload
cargo install img_resize
```

### 下载预编译二进制

访问 [Releases](https://github.com/dwbmio/r_lit/releases) 页面下载适合你平台的二进制文件。

**支持的平台：**
- Linux (x86_64, ARM64)
- macOS (x86_64, ARM64)
- Windows (x86_64)

## 使用说明

### bulk_upload

从 JSON 中提取 URL 并批量上传到 S3：

```bash
# 基本用法
cat data.json | bulk_upload jq -s ~/.s3config -p "images/"

# JSON 输出模式
bulk_upload --json jq -s ~/.s3config < data.json

# 查看帮助
bulk_upload jq --help
```

**配置文件格式** (`.s3`):
```env
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
S3_ENDPOINT=https://s3.example.com
S3_REGION=us-east-1
```

### img_resize

调整图片尺寸或压缩：

```bash
# 等比缩放到最大 800px
img_resize r_resize -m 800 image.jpg

# 精确调整到 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg

# 批量处理目录
img_resize r_resize -m 1024 images/

# 使用 TinyPNG 压缩
img_resize tinyfy images/

# JSON 输出模式
img_resize --json r_resize -m 800 image.jpg

# 查看帮助
img_resize --help
```

## 功能特性

### bulk_upload
- ✅ 自动递归提取 JSON 中的所有 URL
- ✅ URL 自动去重
- ✅ 批量并发下载和上传
- ✅ 支持 S3 兼容存储（MinIO, AWS S3, 阿里云 OSS）
- ✅ JSON 输出模式便于程序解析
- ✅ 详细的进度和错误报告

### img_resize
- ✅ 纯 Rust 实现，无需网络依赖
- ✅ 支持 PNG 和 JPG 格式
- ✅ 三种调整模式：配置文件、等比缩放、精确调整
- ✅ 批量处理目录
- ✅ TinyPNG API 集成
- ✅ JSON 输出模式
- ✅ 保持图片质量

## AI 友好

这些工具专为 AI 调用优化：

- **清晰的 --help 输出**：详细的参数说明和使用示例
- **JSON 输出模式**：结构化数据便于解析
- **标准化错误处理**：明确的错误信息和退出码
- **管道友好**：支持 stdin/stdout 数据流

查看 [TOOL_CATALOG.md](TOOL_CATALOG.md) 了解完整的工具文档。

## 开发

### 构建

```bash
# 构建所有工具
cargo build --release

# 构建单个工具
cd bulk_upload && cargo build --release
cd img_resize && cargo build --release
```

### 测试

```bash
# 运行所有测试
cargo test

# 运行单个工具的测试
cd bulk_upload && cargo test
cd img_resize && cargo test
```

### 发布新版本

```bash
# 更新版本号
# 编辑 bulk_upload/Cargo.toml 和 img_resize/Cargo.toml

# 更新 CHANGELOG.md

# 提交并创建 tag
git add .
git commit -m "chore: bump version to 0.2.0"
git tag -a v0.2.0 -m "Release v0.2.0"
git push origin main
git push origin v0.2.0
```

GitHub Actions 会自动构建并发布所有平台的二进制文件。

查看 [GITHUB_ACTIONS_SETUP.md](GITHUB_ACTIONS_SETUP.md) 了解详细配置。

## 文档

- [工具目录](TOOL_CATALOG.md) - 完整的工具使用文档
- [CLI 优化总结](CLI_OPTIMIZATION_SUMMARY.md) - CLI 优化说明
- [GitHub Actions 配置](GITHUB_ACTIONS_SETUP.md) - CI/CD 配置指南
- [更新日志](CHANGELOG.md) - 版本更新记录

## 系统要求

- **操作系统**：macOS, Linux, Windows
- **架构**：x86_64, ARM64
- **依赖**：无运行时依赖（静态链接）

## 许可证

查看各项目的 LICENSE 文件。

## 贡献

欢迎提交 Issue 和 Pull Request！

---

**注意**：使用前请将 README 中的 `dwbmio` 替换为你的 GitHub 用户名。
