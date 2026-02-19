# img_resize - 图片批量处理工具

批量处理目录内的图片文件，支持缩放、压缩和格式转换。提供纯 Rust 本地处理和 TinyPNG API 压缩两种方式。

---

## 快速开始

```bash
# 按最大像素缩放图片
img_resize r_resize -mx 1000000 input_dir output_dir

# 指定宽度缩放
img_resize r_resize -rw 800 input_dir output_dir

# 强制转换为 JPG 格式
img_resize r_resize -mx 1000000 -j input_dir output_dir

# 使用 TinyPNG 压缩
img_resize tinyfy -k YOUR_API_KEY -i input_dir
```

---

## 命令参考

### `r_resize` - 本地图片缩放和转换

使用纯 Rust 实现的图片处理，支持按像素、宽度、高度缩放，以及格式转换。

```bash
img_resize r_resize [OPTIONS] <INPUT_DIR> <OUTPUT_DIR>
```

| 参数 | 必需 | 说明 |
|------|------|------|
| `<INPUT_DIR>` | 是 | 输入目录路径 |
| `<OUTPUT_DIR>` | 是 | 输出目录路径 |
| `-mx, --max_pixel <N>` | 否 | 最大像素数（保持纵横比） |
| `-rw, --rw <WIDTH>` | 否 | 目标宽度（保持纵横比） |
| `-rh, --rh <HEIGHT>` | 否 | 目标高度（保持纵横比） |
| `-j, --force_jpg` | 否 | 强制转换为 JPG 格式 |
| `-c, --config <FILE>` | 否 | YAML 配置文件路径 |

**支持的图片格式：** JPG/JPEG, PNG, WebP, GIF

### `tinyfy` - TinyPNG API 压缩

使用 TinyPNG 服务压缩图片，通常可减少 50-80% 文件大小。

```bash
img_resize tinyfy -k <API_KEY> -i <INPUT_DIR>
```

| 参数 | 必需 | 说明 |
|------|------|------|
| `-k, --api_key <KEY>` | 是 | TinyPNG API Key |
| `-i, --input <DIR>` | 是 | 输入目录路径 |

---

## 典型场景

### 场景1：批量缩小图片尺寸

将目录内所有图片缩小到 100 万像素以内：

```bash
img_resize r_resize -mx 1000000 photos/ output/
```

### 场景2：统一图片宽度

将所有图片宽度统一为 800px（高度自动计算）：

```bash
img_resize r_resize -rw 800 images/ resized/
```

### 场景3：批量转换为 JPG

将所有图片转换为 JPG 格式并缩放：

```bash
img_resize r_resize -mx 500000 -j input/ output/
```

### 场景4：使用配置文件批量处理

创建 `config.yaml`：

```yaml
tasks:
  - input: /path/to/photos
    output: /path/to/output
    max_pixel: 1000000
    force_jpg: true
  - input: /path/to/images
    output: /path/to/thumbnails
    max_pixel: 100000
```

执行批量任务：

```bash
img_resize r_resize -c config.yaml
```

### 场景5：使用 TinyPNG 压缩

```bash
img_resize tinyfy -k your-api-key-here -i photos/
```

---

## 缩放算法说明

### 按最大像素缩放 (`-mx`)

保持纵横比，将图片总像素数限制在指定值以内。

**示例：**
- 原图：1920x1080 (2,073,600 像素)
- 参数：`-mx 1000000`
- 结果：约 1066x600 (1,000,000 像素)

### 按宽度缩放 (`-rw`)

固定宽度，高度按比例自动计算。

**示例：**
- 原图：1920x1080
- 参数：`-rw 800`
- 结果：800x450

### 按高度缩放 (`-rh`)

固定高度，宽度按比例自动计算。

**示例：**
- 原图：1920x1080
- 参数：`-rh 600`
- 结果：1067x600

---

## 配置文件格式

### YAML 配置文件

```yaml
tasks:
  - input: /path/to/input1
    output: /path/to/output1
    max_pixel: 1000000
    force_jpg: true

  - input: /path/to/input2
    output: /path/to/output2
    width: 800

  - input: /path/to/input3
    output: /path/to/output3
    height: 600
```

**字段说明：**
- `input`: 输入目录（必需）
- `output`: 输出目录（必需）
- `max_pixel`: 最大像素数（可选）
- `width`: 目标宽度（可选）
- `height`: 目标高度（可选）
- `force_jpg`: 是否强制转换为 JPG（可选，默认 false）

---

## 文件过滤规则

工具会自动过滤：
- ✅ 处理：`.jpg`, `.jpeg`, `.png`, `.webp`, `.gif` 文件
- ❌ 跳过：隐藏文件（以 `.` 开头）
- ❌ 跳过：非图片文件
- ✅ 大小写不敏感（`.JPG` 和 `.jpg` 都会处理）
