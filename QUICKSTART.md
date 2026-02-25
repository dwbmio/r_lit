# 快速开始指南

## 前置准备

在开始之前，你需要：

1. **GitHub 账户**
2. **Git 已安装**
3. **Rust 工具链已安装**（用于本地测试）

## 步骤 1：配置 GitHub 仓库

### 1.1 更新仓库信息

编辑以下文件，将 `YOUR_USERNAME` 替换为你的 GitHub 用户名：

- `README.md`
- `install.sh`（第 11 行）

```bash
# 使用 sed 批量替换（macOS）
sed -i '' 's/YOUR_USERNAME/你的用户名/g' README.md install.sh

# 或者使用 sed 批量替换（Linux）
sed -i 's/YOUR_USERNAME/你的用户名/g' README.md install.sh
```

### 1.2 提交更改

```bash
git add .
git commit -m "chore: setup GitHub Actions and documentation"
git push origin main
```

## 步骤 2：测试本地构建

在推送到 GitHub 之前，先在本地测试：

```bash
# 测试 bulk_upload
cd bulk_upload
cargo build --release
./target/release/bulk_upload --help

# 测试 img_resize
cd ../img_resize
cargo build --release
./target/release/img_resize --help
```

## 步骤 3：测试 CI Workflow

推送代码后，GitHub Actions 会自动运行 CI：

1. 访问 `https://github.com/YOUR_USERNAME/r_lit/actions`
2. 查看 "CI" workflow 是否成功
3. 如果失败，查看日志并修复问题

## 步骤 4：创建第一个 Release

### 4.1 更新版本号

编辑 `bulk_upload/Cargo.toml`：
```toml
[package]
name = "bulk_upload"
version = "0.2.0"  # 更新这里
```

编辑 `img_resize/Cargo.toml`：
```toml
[package]
name = "img_resize"
version = "0.2.0"  # 更新这里
```

### 4.2 更新 CHANGELOG

编辑 `CHANGELOG.md`，确保有当前版本的条目。

### 4.3 创建 Tag 并推送

```bash
# 提交版本更新
git add .
git commit -m "chore: bump version to 0.2.0"

# 创建 tag
git tag -a v0.2.0 -m "Release v0.2.0

## Changes
- Add GitHub Actions workflow
- Add JSON output mode
- Improve CLI documentation
"

# 推送到 GitHub
git push origin main
git push origin v0.2.0
```

### 4.4 等待构建完成

1. 访问 Actions 页面
2. 查看 "Release" workflow 进度
3. 预计等待 30-35 分钟

### 4.5 验证 Release

构建完成后：

1. 访问 `https://github.com/YOUR_USERNAME/r_lit/releases`
2. 确认 v0.2.0 release 已创建
3. 检查是否有以下文件：
   - `bulk_upload-x86_64-unknown-linux-gnu.tar.gz`
   - `bulk_upload-aarch64-unknown-linux-gnu.tar.gz`
   - `bulk_upload-x86_64-apple-darwin.tar.gz`
   - `bulk_upload-aarch64-apple-darwin.tar.gz`
   - `bulk_upload-x86_64-pc-windows-gnu.zip`
   - `img_resize-*` (相同的平台)
   - `SHA256SUMS`

## 步骤 5：测试安装脚本

### 5.1 本地测试

```bash
# 下载并运行安装脚本
curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/r_lit/main/install.sh | sh

# 或者先下载再运行
curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/r_lit/main/install.sh -o install.sh
chmod +x install.sh
./install.sh
```

### 5.2 验证安装

```bash
# 检查工具是否安装成功
bulk_upload --version
img_resize --version

# 测试基本功能
bulk_upload --help
img_resize --help
```

## 步骤 6：（可选）发布到 crates.io

如果你想让用户通过 `cargo install` 安装：

### 6.1 创建 crates.io 账户

1. 访问 https://crates.io/
2. 使用 GitHub 账户登录
3. 访问 https://crates.io/settings/tokens
4. 创建新的 API token

### 6.2 添加 GitHub Secret

1. 访问 `https://github.com/YOUR_USERNAME/r_lit/settings/secrets/actions`
2. 点击 "New repository secret"
3. 名称：`CARGO_TOKEN`
4. 值：粘贴你的 crates.io API token
5. 点击 "Add secret"

### 6.3 更新 Cargo.toml

确保两个项目的 `Cargo.toml` 包含必要的 metadata：

```toml
[package]
name = "bulk_upload"
version = "0.2.0"
edition = "2021"
authors = ["Your Name <your.email@example.com>"]
description = "批量下载 URL 并上传到 S3 对象存储"
license = "MIT OR Apache-2.0"
repository = "https://github.com/YOUR_USERNAME/r_lit"
homepage = "https://github.com/YOUR_USERNAME/r_lit"
keywords = ["s3", "upload", "cli", "batch"]
categories = ["command-line-utilities"]
```

### 6.4 手动发布（首次）

```bash
# 发布 bulk_upload
cd bulk_upload
cargo publish

# 发布 img_resize
cd ../img_resize
cargo publish
```

之后的版本会通过 GitHub Actions 自动发布。

## 常见问题

### Q: 构建失败怎么办？

A: 查看 Actions 日志，常见问题：
- OpenSSL 依赖：考虑使用 `rustls`
- 交叉编译失败：检查 `cross` 工具是否支持目标平台

### Q: 如何跳过 crates.io 发布？

A: 删除或注释掉 `.github/workflows/release.yml` 中的 `publish-crates` job。

### Q: 如何只构建特定平台？

A: 编辑 `.github/workflows/release.yml`，删除不需要的 matrix 条目。

### Q: 构建时间太长怎么办？

A:
1. 启用更激进的缓存
2. 减少构建的平台数量
3. 使用 `sccache`

### Q: 如何测试 workflow 而不创建 release？

A: 使用 workflow_dispatch 手动触发，或者推送到测试分支。

## 下一步

- 添加更多测试
- 优化构建时间
- 添加更多平台支持
- 设置 CI 徽章
- 编写使用教程

## 获取帮助

如果遇到问题：

1. 查看 [GITHUB_ACTIONS_SETUP.md](GITHUB_ACTIONS_SETUP.md)
2. 查看 GitHub Actions 日志
3. 搜索相关错误信息
4. 提交 Issue

---

祝你发布顺利！🚀
