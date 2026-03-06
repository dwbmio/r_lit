# bulk_upload

> 从 JSON 批量提取 URL，并发下载后上传到 S3 兼容存储。

## 快速开始

```bash
# 安装
curl -fsSL https://nexus.gamesci-lite.com/repository/raw-prod/r_lit/bulk_upload/install.sh | TOOL_NAME=bulk_upload bash

# 创建配置
cat > .s3 << EOF
S3_BUCKET=my-bucket
S3_ACCESS_KEY=your-key
S3_SECRET_KEY=your-secret
S3_ENDPOINT=https://your-account.r2.cloudflarestorage.com
S3_REGION=auto
EOF

# 上传
cat urls.json | bulk_upload jq - --s3 .s3 --prefix assets/ --concurrency 10
```

## 功能

输入一段 JSON，自动递归提取其中所有 URL，并发下载后上传到 S3 存储桶。

```bash
# 直接传 JSON
bulk_upload jq '{"files":["https://example.com/a.png","https://example.com/b.png"]}' \
  --s3 .s3 --prefix textures/

# 从 stdin 读取
curl -s https://api.example.com/assets | bulk_upload jq - --s3 .s3
```

## 支持的存储

AWS S3 · Cloudflare R2 · Aliyun OSS · iDrive E2 · MinIO · 任何 S3 兼容存储

## 平台

| 平台 | 架构 | 大小 |
|------|------|------|
| Linux | x86_64 | ~4MB |
| macOS | Apple Silicon | ~3MB |

## 从源码构建

```bash
cargo build --release
```

## AI 友好描述

结构化工具描述见 [llms.txt](./llms.txt) / [llms_cn.txt](./llms_cn.txt)。

## License

MIT
