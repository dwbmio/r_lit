# bulk_upload - 批量下载 URL 并上传到 S3

从 JSON 文件中提取所有 HTTP(S) URL，批量并发下载后上传到 S3 兼容对象存储（支持 MinIO、Cloudflare R2 等）。

---

## 快速开始

```bash
# 从 JSON 文件提取 URL 并上传到 S3
bulk_upload jq urls.json --s3 /path/to/.s3 --prefix assets/images/

# 通过管道传入 JSON
cat data.json | bulk_upload jq --s3 /path/to/.s3 --prefix assets/

# 从 API 获取 JSON 并处理
curl -s https://api.example.com/data | bulk_upload jq --s3 /path/to/.s3
```

---

## 命令参考

### `jq` - 从 JSON 提取 URL 并上传

从任意 JSON 结构中递归提取所有 HTTP(S) URL，批量并发下载后上传到 S3。

```bash
bulk_upload jq [JSON_TEXT] --s3 <CONFIG> [OPTIONS]
```

| 参数 | 必需 | 说明 |
|------|------|------|
| `JSON_TEXT` | 否 | JSON 文本内容，省略则从 stdin 读取 |
| `--s3 <CONFIG>` | 是 | S3 配置文件路径（dotenv 格式） |
| `--prefix <PREFIX>` | 否 | S3 上传目标前缀路径（默认空） |
| `--concurrency <N>` | 否 | 每批并发下载/上传数量（默认 10） |

**工作流程：**
```
加载 S3 配置 → 递归提取 URL → 去重 → 分批并发下载 → 并发上传到 S3
```

---

## 典型场景

### 场景1：处理本地 JSON 文件

```bash
bulk_upload jq urls.json --s3 ~/.s3config --prefix assets/images/ --concurrency 20
```

### 场景2：从 API 获取数据并处理

```bash
curl -s https://api.example.com/books | bulk_upload jq --s3 ~/.s3config --prefix covers/
```

### 场景3：处理嵌套 JSON 结构

工具会自动递归遍历任意深度的 JSON 结构，提取所有 HTTP(S) URL：

```json
{
  "data": {
    "items": [
      {"image": "https://example.com/1.jpg"},
      {"nested": {"url": "https://example.com/2.jpg"}}
    ]
  }
}
```

所有 URL 都会被提取并上传。

### 场景4：上传到 Cloudflare R2

```bash
# .s3 配置文件示例
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-access-key
S3_SECRET_KEY=your-secret-key
S3_ENDPOINT=https://account-id.r2.cloudflarestorage.com
S3_REGION=auto

bulk_upload jq data.json --s3 .s3 --prefix images/
```

---

## 配置文件格式

### S3 配置文件（dotenv 格式）

创建 `.s3` 配置文件：

```env
S3_BUCKET=my-bucket
S3_ACCESS_KEY=xxxxx
S3_SECRET_KEY=xxxxx
S3_ENDPOINT=https://s3.example.com
S3_REGION=us-east-1  # 可选，默认 us-east-1
```

**支持的 S3 服务：**
- AWS S3
- MinIO
- Cloudflare R2
- 其他 S3 兼容存储

---

## S3 Key 生成规则

上传文件的 S3 key 格式：`{prefix}/{filename}`

- `filename` 从 URL 最后一段路径提取
- 自动去除 query 参数

**示例：**

| URL | Prefix | S3 Key |
|-----|--------|--------|
| `https://example.com/image.jpg` | `assets/` | `assets/image.jpg` |
| `https://example.com/photo.jpg?size=large` | `images/` | `images/photo.jpg` |
| `https://example.com/path/to/file.png` | `` | `file.png` |
