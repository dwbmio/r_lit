# GitHub Actions 配置指南

## 设置步骤

### 1. 配置 GitHub Secrets

如果你想发布到 crates.io，需要添加 CARGO_TOKEN：

1. 访问 https://crates.io/settings/tokens
2. 创建新的 API token
3. 在 GitHub 仓库设置中添加 Secret：
   - 名称：`CARGO_TOKEN`
   - 值：你的 crates.io API token

### 2. 更新安装脚本

编辑 `install.sh`，将 `YOUR_GITHUB_USERNAME` 替换为你的 GitHub 用户名：

```bash
REPO="YOUR_GITHUB_USERNAME/r_lit"
```

### 3. 发布新版本

#### 方式 1：通过 Git Tag（推荐）

```bash
# 更新版本号
cd bulk_upload
# 编辑 Cargo.toml，更新 version = "0.2.0"

cd ../img_resize
# 编辑 Cargo.toml，更新 version = "0.2.0"

# 提交更改
git add .
git commit -m "chore: bump version to 0.2.0"

# 创建 tag
git tag -a v0.2.0 -m "Release v0.2.0"

# 推送到 GitHub
git push origin main
git push origin v0.2.0
```

#### 方式 2：手动触发

1. 访问 GitHub Actions 页面
2. 选择 "Release" workflow
3. 点击 "Run workflow"
4. 输入版本号（如 v0.2.0）
5. 点击 "Run workflow"

### 4. 验证发布

发布完成后，检查：

1. **GitHub Releases**：
   - 访问 `https://github.com/YOUR_USERNAME/r_lit/releases`
   - 确认新版本已创建
   - 下载并测试二进制文件

2. **安装脚本测试**：
   ```bash
   curl -fsSL https://raw.githubusercontent.com/YOUR_USERNAME/r_lit/main/install.sh | sh
   ```

3. **crates.io**（如果启用）：
   - 访问 https://crates.io/crates/bulk_upload
   - 访问 https://crates.io/crates/img_resize

## Workflow 说明

### CI Workflow (`.github/workflows/ci.yml`)

**触发条件：**
- Push 到 main/master/develop 分支
- Pull Request 到 main/master/develop 分支

**执行内容：**
- 运行测试
- 代码格式检查（rustfmt）
- Lint 检查（clippy）
- 构建 Linux 版本

**预估时间：** 5-8 分钟
**预估成本：** 免费（使用免费额度）

### Release Workflow (`.github/workflows/release.yml`)

**触发条件：**
- Push tag（格式：v*.*.*）
- 手动触发

**执行内容：**
1. **构建阶段**（并行）：
   - Linux x86_64
   - Linux ARM64
   - macOS x86_64
   - macOS ARM64
   - Windows x86_64

2. **发布阶段**：
   - 创建 GitHub Release
   - 上传所有二进制文件
   - 生成 SHA256 校验和
   - 提取 CHANGELOG

3. **发布到 crates.io**（可选）：
   - 发布 bulk_upload
   - 发布 img_resize

**预估时间：** 30-35 分钟
**预估成本：** $0.30-0.35/次（使用 cross 交叉编译）

## 成本优化

### 当前配置（最优）

- **CI**：仅 Linux 构建，完全免费
- **Release**：Linux 交叉编译所有平台
- **预估月成本**：$0.66（假设月发版 2 次）
- **预估年成本**：$7.92

### 免费额度使用

GitHub 免费账户提供：
- 2,000 分钟/月（Linux 倍率）

当前配置每次发版消耗：
- CI：~8 分钟
- Release：~33 分钟
- **总计：~41 分钟/次**

**每月可免费发版次数：** 2,000 / 41 ≈ 48 次

### 进一步优化

如果需要降低成本：

1. **启用更激进的缓存**：
   ```yaml
   - uses: Swatinem/rust-cache@v2
     with:
       cache-all-crates: true
       save-if: ${{ github.ref == 'refs/heads/main' }}
   ```

2. **仅在 tag 时构建所有平台**：
   ```yaml
   # CI 只构建 Linux
   # Release 构建所有平台
   ```

3. **使用 sccache**：
   ```yaml
   - name: Setup sccache
     uses: mozilla-actions/sccache-action@v0.0.3
   ```

## 故障排查

### 构建失败

1. **检查 cross 兼容性**：
   ```bash
   # 本地测试
   cargo install cross
   cross build --target x86_64-pc-windows-gnu
   ```

2. **查看构建日志**：
   - GitHub Actions 页面 → 失败的 workflow → 查看详细日志

3. **常见问题**：
   - OpenSSL 依赖：考虑使用 `rustls` 替代
   - 系统库依赖：使用静态链接

### 发布失败

1. **权限问题**：
   - 确保 workflow 有 `contents: write` 权限
   - 检查 GITHUB_TOKEN 是否有效

2. **Tag 格式**：
   - 必须是 `v*.*.*` 格式（如 v0.2.0）
   - 不要使用 `0.2.0`（缺少 v 前缀）

3. **crates.io 发布失败**：
   - 检查 CARGO_TOKEN 是否正确
   - 确保包名未被占用
   - 检查 Cargo.toml 中的 metadata

## 监控和通知

### 设置通知

1. **GitHub 通知**：
   - Settings → Notifications → Actions
   - 启用失败通知

2. **Slack/Discord 集成**（可选）：
   ```yaml
   - name: Notify on failure
     if: failure()
     uses: 8398a7/action-slack@v3
     with:
       status: ${{ job.status }}
       webhook_url: ${{ secrets.SLACK_WEBHOOK }}
   ```

### 查看使用情况

1. 访问 Settings → Billing → Usage this month
2. 查看 Actions 分钟数消耗
3. 按 workflow 和 runner 类型分类

## 下一步

1. **测试 workflow**：
   ```bash
   # 创建测试 tag
   git tag -a v0.2.0-test -m "Test release"
   git push origin v0.2.0-test
   ```

2. **更新文档**：
   - 在 README.md 中添加安装说明
   - 更新 TOOL_CATALOG.md

3. **添加徽章**：
   ```markdown
   ![CI](https://github.com/YOUR_USERNAME/r_lit/workflows/CI/badge.svg)
   ![Release](https://github.com/YOUR_USERNAME/r_lit/workflows/Release/badge.svg)
   ```
