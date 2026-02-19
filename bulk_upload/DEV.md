# bulk_upload 开发文档

> 📌 **基础规则:** 遵循 [`_base_rust_cli.md`](../ci-all-in-one/_ai/rlit-dev/_base_rust_cli.md)

---

## 编码规范

### 错误处理

1. **禁止使用 `unwrap()`** — 所有可能失败的操作必须通过 `?` 传播或显式 `match` / `if let` 处理
2. **所有错误归档到 `error.rs`** — 基于 `thiserror` 的 `AppError` 枚举
3. **`panic!()` 禁止直接使用**
4. **`expect()` 需附带明确描述** — 说明为什么此处不应该失败

---

## 项目结构

```
bulk_upload/
├── src/
│   ├── main.rs            # clap derive 入口
│   ├── error.rs           # AppError 枚举
│   └── subcmd/
│       ├── mod.rs         # 子命令注册
│       └── jq.rs          # 核心逻辑：S3 配置、URL 提取、下载上传
└── tests/                 # 单元测试
    ├── s3_config_tests.rs
    ├── url_extraction_tests.rs
    └── s3_key_tests.rs
```

### 核心模块说明

**src/subcmd/jq.rs:**
- `load_s3_config()` - 从 dotenv 文件加载 S3 配置
- `extract_urls()` - 递归遍历 JSON 提取所有 HTTP(S) URL
- `build_s3_client()` - 构建 S3 客户端（兼容 MinIO）
- `download_file()` - 使用 reqwest 下载单个文件
- `upload_to_s3()` - AWS SDK 上传到 S3
- `build_s3_key()` - 从 URL 提取文件名并拼接前缀

---

## 测试

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试模块
cargo test s3_config_tests
cargo test url_extraction_tests
cargo test s3_key_tests

# 显示测试输出
cargo test -- --nocapture
```

### 测试覆盖

**tests/s3_config_tests.rs:**
- ✅ 有效配置解析
- ✅ 缺失字段处理
- ✅ 默认 region 值
- ✅ 注释和空白处理
- ✅ 空值和空文件处理

**tests/url_extraction_tests.rs:**
- ✅ 简单 URL 提取
- ✅ 嵌套 JSON 结构
- ✅ 混合内容过滤
- ✅ HTTP/HTTPS 协议支持
- ✅ 深层嵌套结构
- ✅ 无效 URL 格式过滤

**tests/s3_key_tests.rs:**
- ✅ 简单 URL 文件名提取
- ✅ Query 参数去除
- ✅ 前缀处理（空前缀、尾部斜杠）
- ✅ 复杂文件名和中文文件名
- ✅ 嵌套路径处理

---

## 依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `clap` | 4 (derive) | CLI 框架 |
| `tokio` | 1 (rt-multi-thread) | 异步运行时 |
| `thiserror` | 2 | 错误处理 |
| `serde` / `serde_json` | 1 | JSON 解析 |
| `reqwest` | 0.13 (stream) | HTTP 下载 |
| `aws-sdk-s3` | 1 | S3 上传 |
| `aws-config` | 1 | AWS 配置 |
| `aws-credential-types` | 1 | AWS 凭证 |
| `futures` | 0.3 | 并发 join_all |
| `log` / `fern` | - | 日志 |

### Dev Dependencies

| 依赖 | 版本 | 用途 |
|------|------|------|
| `tempfile` | 3 | 测试中创建临时文件 |

---

## 构建和发布

### 本地开发

```bash
# 构建 debug 版本
cargo build

# 本地安装（macOS: /usr/local/bin）
just install_loc

# 运行
bulk_upload jq test.json --s3 .s3
```

### CI/CD

通过 Jenkins Pipeline 构建发布：
- Job: `r_lit-binary-build`
- 参数: `TOOL_NAME=bulk_upload`
- 产物: Nexus `raw-prod/r_lit/bulk_upload/`

详见 [`_base_rust_cli.md`](../ci-all-in-one/_ai/rlit-dev/_base_rust_cli.md) §5 CI/CD 规范。
