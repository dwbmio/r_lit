# R_LIT 工具目录

跨平台 CLI 工具集，用于图片处理和文件上传。

## 安装

```bash
# macOS/Linux
curl -fsSL https://your-domain.com/install.sh | sh

# 或使用 cargo
cargo install bulk_upload
cargo install img_resize
```

---

## bulk_upload

批量下载 URL 并上传到 S3 对象存储。

### 命令：`bulk_upload jq`

从 JSON 中提取所有 URL，批量下载并上传到 S3。

**用法：**
```bash
bulk_upload jq [JSON_TEXT] -s <S3_CONFIG> [-p <PREFIX>] [-c <CONCURRENCY>]
```

**参数：**
- `[JSON_TEXT]` - JSON 文本内容（可选，省略时从 stdin 读取）
- `-s, --s3 <S3_CONFIG>` - .s3 配置文件路径（必需）
  - 格式：dotenv，包含 `S3_BUCKET`, `S3_ACCESS_KEY`, `S3_SECRET_KEY`, `S3_ENDPOINT`, `S3_REGION`
- `-p, --prefix <PREFIX>` - S3 上传前缀路径（默认：空）
- `-c, --concurrency <CONCURRENCY>` - 并发数（默认：10，范围：1-100）

**示例：**
```bash
# 从文件读取 JSON
cat data.json | bulk_upload jq -s ~/.s3config -p "images/" -c 20

# 直接传入 JSON
bulk_upload jq '{"urls":["https://example.com/1.jpg"]}' -s ~/.s3config

# 管道传入
curl https://api.example.com/data | bulk_upload jq -s ~/.s3config -p "assets/"
```

**输出：**
- 成功：上传的文件列表和 S3 路径
- 失败：错误信息和失败的 URL

---

## img_resize

图片尺寸调整和压缩工具。

### 命令 1：`img_resize r_resize`

使用纯 Rust 调整图片尺寸（无需网络）。

**用法：**
```bash
img_resize r_resize [OPTIONS] <PATH>
```

**参数：**
- `<PATH>` - 图片文件或目录路径（必需）
- `-c, --resize_config <FILE>` - YAML 配置文件路径
- `-m, --max_pixel <SIZE>` - 最大像素值（等比缩放）
- `--rw <WIDTH>` - 目标宽度（需配合 --rh）
- `--rh <HEIGHT>` - 目标高度（需配合 --rw）
- `-j, --force_jpg <BOOL>` - 强制转换为 JPG

**参数规则：**
- `--resize_config` 与其他尺寸参数互斥
- `--max_pixel` 与 `--rw/--rh` 互斥
- `--rw` 和 `--rh` 必须同时使用

**示例：**
```bash
# 等比缩放到最大 800px
img_resize r_resize -m 800 /path/to/image.jpg

# 精确调整到 1920x1080
img_resize r_resize --rw 1920 --rh 1080 /path/to/image.jpg

# 批量处理目录
img_resize r_resize -m 1024 /path/to/images/

# 使用配置文件
img_resize r_resize -c resize.yaml /path/to/image.jpg

# 强制转换为 JPG
img_resize r_resize -j true /path/to/image.png
```

**配置文件格式（YAML）：**
```yaml
vec_size:
  - [1920, 1080]
  - [800, 600]
vec_f:
  - "output/large.png"
  - "output/small.png"
base_f: "/output/base/path"
```

### 命令 2：`img_resize tinyfy`

使用 TinyPNG API 压缩图片。

**用法：**
```bash
img_resize tinyfy [-d <DO_SIZE_PERF>] <PATH>
```

**参数：**
- `<PATH>` - 图片文件或目录路径（必需）
- `-d, --do_size_perf <BOOL>` - 执行最佳尺寸优化（可选）

**示例：**
```bash
# 压缩单个文件
img_resize tinyfy /path/to/image.jpg

# 批量压缩目录
img_resize tinyfy /path/to/images/

# 启用尺寸优化
img_resize tinyfy -d true /path/to/image.png
```

**注意：** 需要设置 TinyPNG API Key 环境变量。

---

## 通用特性

**支持的图片格式：**
- PNG
- JPG/JPEG

**日志输出：**
- Debug 模式：详细日志
- Release 模式：仅 Info 级别

**错误处理：**
- 非零退出码表示失败
- 错误信息输出到 stderr
- 成功信息输出到 stdout

**性能优化：**
- 并发处理（bulk_upload）
- 批量操作支持
- 优化的二进制大小（strip + LTO）

---

## 系统要求

- **操作系统：** macOS, Linux, Windows
- **架构：** x86_64, ARM64
- **依赖：** 无运行时依赖（静态链接）

## 版本

- `bulk_upload`: v0.2.0-alpha.1
- `img_resize`: v0.1.2

## 许可证

查看各项目的 LICENSE 文件。
