# img_resize

> 纯 Rust 图片缩放与压缩工具，无网络依赖。

## 快速开始

```bash
# 等比缩放到最大 800px
img_resize r_resize -m 800 image.jpg

# 精确调整到 1920x1080
img_resize r_resize --rw 1920 --rh 1080 image.jpg

# 批量处理目录
img_resize r_resize -m 1024 images/

# 强制转为 JPG
img_resize r_resize -m 800 -j image.png
```

## 缩放模式

### 1. 等比缩放 (`-m`)

宽高均不超过指定像素值，保持原始宽高比。

```bash
img_resize r_resize -m 800 photo.jpg
```

### 2. 精确调整 (`--rw` + `--rh`)

指定精确的宽度和高度，不保持宽高比。

```bash
img_resize r_resize --rw 1920 --rh 1080 photo.jpg
```

### 3. 配置文件模式 (`-c`)

使用 YAML 配置文件一次生成多个尺寸。

```yaml
vec_size:
  - [1920, 1080]
  - [800, 600]
vec_f:
  - "output/large.png"
  - "output/small.png"
base_f: "/output/base/path"
```

```bash
img_resize r_resize -c config.yaml input.png
```

## 支持格式

- PNG
- JPG / JPEG

## JSON 输出

```bash
img_resize --json r_resize -m 800 image.jpg
```

返回结构化 JSON，便于程序解析。

## 从源码构建

```bash
cargo build --release
```

## 备注

`tinyfy` 子命令因 OpenSSL/musl 兼容性问题暂时禁用。

## License

MIT
