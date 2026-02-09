# bulk_upload 项目结构与子命令

> 📌 **基础规则:** 本项目遵循 [`_base_rust_cli.md`](../ci-all-in-one/_ai/backend/_base_rust_cli.md)

## 编码规范

### 错误处理

1. **禁止使用 `unwrap()`** — 所有可能失败的操作必须通过 `?` 传播或显式 `match` / `if let` 处理。
2. **所有非预期错误必须归档到 `error.rs` 的 `AppError` 枚举中**（基于 `thiserror`），新增错误场景时需同步扩展枚举变体。
3. **`panic!()` 禁止直接使用。**
4. **`expect()` 是唯一允许的 panic 方式**，且必须附带明确的预期错误描述，说明"为什么此处不应该失败"。
5. 优先使用 `?` 操作符 + `AppError` 变体做错误传播，`expect()` 仅用于"逻辑上不可能失败"的场景。

---

## 项目结构

```
bulk_upload/
├── .justfile              # 构建/安装自动化
├── .gitignore
├── Cargo.toml             # 项目配置 (clap derive + tokio + aws-sdk-s3)
├── Cargo.lock
├── LLVM.md                # 本文件
├── src/
│   ├── main.rs            # 程序入口，clap derive macro 命令定义
│   ├── error.rs           # AppError 错误枚举
│   └── subcmd/
│       ├── mod.rs          # 子命令注册
│       └── jp.rs           # jp 子命令：JSON URL 批量下载 + S3 上传
```

## 子命令列表

| 子命令 | 说明 | 参数 |
|--------|------|------|
| `jp` | 从 JSON 文件解析 URL 列表，分批并发下载后上传到 S3 | `json_path`（必需）JSON 文件路径<br>`-s, --s3` .s3 配置文件的绝对路径<br>`-p, --prefix` S3 目标前缀路径（默认空）<br>`-c, --concurrency` 每批并发数（默认 10） |

### jp 子命令工作流程

```
加载 .s3 配置 → 读取 JSON 文件 → 解析 URL 数组 → 分批(concurrency) → 并发下载 → 并发上传 S3
```

**使用示例：**

```bash
bulk_upload jp urls.json --s3 /path/to/.s3 --prefix assets/images/ -c 20
```

**.s3 配置文件格式**（dotenv 风格，与 hfrog-cli 等项目通用）：

```
S3_BUCKET=my-bucket
S3_ACCESS_KEY=xxxxx
S3_SECRET_KEY=xxxxx
S3_ENDPOINT=http://minio.example.com:9000
S3_REGION=us-east-1
```

**JSON 文件格式：**

支持两种格式：

1. 带 `books` 数组的对象（如当当书籍列表），从 `books[].image` 提取 URL：

```json
{
  "books": [
    { "title": "...", "image": "https://example.com/cover1.jpg" },
    { "title": "...", "image": "https://example.com/cover2.jpg" }
  ]
}
```

> 空 `image` 字段会被自动跳过。

**S3 key 生成规则：** `{prefix}/{filename}`，filename 从 URL 最后一段路径提取（去除 query 参数）。

## 关键依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `clap` | 4 (derive) | CLI 框架（derive macro 模式） |
| `tokio` | 1 (rt-multi-thread) | 异步运行时 |
| `thiserror` | 2 | 错误处理 |
| `serde` / `serde_json` | 1 | JSON 解析 |
| `reqwest` | 0.12 (stream) | HTTP 下载 |
| `aws-sdk-s3` | 1 | S3 上传 |
| `aws-config` | 1 | AWS 配置 |
| `futures` | 0.3 | 并发 join_all |
| `log` / `fern` | - | 日志 |
