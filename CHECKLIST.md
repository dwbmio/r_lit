# 发布前检查清单

在推送到 GitHub 并创建第一个 release 之前，请完成以下检查：

## 必须完成 ✅

### 1. 配置文件更新

- [ ] 将 `README.md` 中的 `YOUR_USERNAME` 替换为你的 GitHub 用户名
- [ ] 将 `install.sh` 中的 `YOUR_USERNAME` 替换为你的 GitHub 用户名（第 11 行）

**快速替换命令：**
```bash
# macOS
sed -i '' 's/YOUR_USERNAME/你的用户名/g' README.md install.sh

# Linux
sed -i 's/YOUR_USERNAME/你的用户名/g' README.md install.sh
```

### 2. 版本号检查

- [ ] `bulk_upload/Cargo.toml` 版本号正确
- [ ] `img_resize/Cargo.toml` 版本号正确
- [ ] `CHANGELOG.md` 包含当前版本的条目

### 3. 本地测试

- [ ] `bulk_upload` 编译成功
  ```bash
  cd bulk_upload && cargo build --release
  ```
- [ ] `img_resize` 编译成功
  ```bash
  cd img_resize && cargo build --release
  ```
- [ ] `bulk_upload --help` 输出正确
- [ ] `img_resize --help` 输出正确
- [ ] 测试通过
  ```bash
  cargo test
  ```

### 4. Git 配置

- [ ] 已初始化 Git 仓库
  ```bash
  git init
  ```
- [ ] 已添加 remote
  ```bash
  git remote add origin https://github.com/YOUR_USERNAME/r_lit.git
  ```
- [ ] 已提交所有更改
  ```bash
  git add .
  git commit -m "chore: setup GitHub Actions and documentation"
  ```

## 可选但推荐 ⭐

### 5. 文档完善

- [ ] 更新 `README.md` 添加项目描述
- [ ] 添加 LICENSE 文件
- [ ] 添加 `.gitignore` 文件（如果还没有）
- [ ] 检查所有文档链接是否正确

### 6. 代码质量

- [ ] 运行 `cargo fmt` 格式化代码
- [ ] 运行 `cargo clippy` 检查警告
- [ ] 修复所有 clippy 警告

### 7. 安全检查

- [ ] 确保没有硬编码的密钥或敏感信息
- [ ] 检查 `.gitignore` 是否包含敏感文件
- [ ] 确认 TinyPNG API key 不在代码中（应该从环境变量读取）

## 首次发布步骤

完成上述检查后：

### 1. 推送到 GitHub

```bash
git push -u origin main
```

### 2. 验证 CI

1. 访问 `https://github.com/YOUR_USERNAME/r_lit/actions`
2. 确认 CI workflow 运行成功
3. 如果失败，查看日志并修复

### 3. 创建第一个 Release

```bash
# 创建 tag
git tag -a v0.2.0 -m "Release v0.2.0

## Features
- GitHub Actions workflow for automated releases
- Cross-platform binary builds (Linux, macOS, Windows)
- JSON output mode for both tools
- Improved CLI documentation

## Tools
- bulk_upload v0.2.0
- img_resize v0.2.0
"

# 推送 tag
git push origin v0.2.0
```

### 4. 等待构建

- 访问 Actions 页面
- 查看 Release workflow 进度
- 预计等待 30-35 分钟

### 5. 验证 Release

1. 访问 `https://github.com/YOUR_USERNAME/r_lit/releases`
2. 确认 v0.2.0 已创建
3. 检查所有平台的二进制文件都已上传
4. 下载并测试一个二进制文件

### 6. 测试安装脚本

```bash
curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/r_lit/main/install.sh | sh
```

## 可选：发布到 crates.io

如果你想让用户通过 `cargo install` 安装：

### 1. 准备 Cargo.toml

确保包含必要的 metadata：

```toml
[package]
name = "bulk_upload"
version = "0.2.0"
authors = ["Your Name <your.email@example.com>"]
description = "批量下载 URL 并上传到 S3 对象存储"
license = "MIT OR Apache-2.0"
repository = "https://github.com/YOUR_USERNAME/r_lit"
keywords = ["s3", "upload", "cli"]
categories = ["command-line-utilities"]
```

### 2. 创建 crates.io Token

1. 访问 https://crates.io/settings/tokens
2. 创建新 token
3. 添加到 GitHub Secrets（名称：`CARGO_TOKEN`）

### 3. 手动首次发布

```bash
cd bulk_upload
cargo publish

cd ../img_resize
cargo publish
```

之后的版本会自动发布。

## 故障排查

### CI 失败

- 查看 Actions 日志
- 检查代码格式：`cargo fmt --check`
- 检查 clippy：`cargo clippy`
- 本地运行测试：`cargo test`

### Release 构建失败

- 检查 cross 是否支持目标平台
- 查看构建日志中的错误信息
- 尝试本地交叉编译：`cross build --target x86_64-pc-windows-gnu`

### 安装脚本失败

- 确认 GitHub 用户名已正确替换
- 检查 Release 是否已创建
- 确认二进制文件已上传

## 完成后

- [ ] 在 README 中添加 CI 徽章
- [ ] 分享你的项目
- [ ] 收集用户反馈
- [ ] 计划下一个版本

## 获取帮助

如果遇到问题：

1. 查看 [QUICKSTART.md](QUICKSTART.md)
2. 查看 [GITHUB_ACTIONS_SETUP.md](GITHUB_ACTIONS_SETUP.md)
3. 查看 GitHub Actions 日志
4. 搜索相关错误信息

---

准备好了吗？开始你的第一次发布吧！🚀
