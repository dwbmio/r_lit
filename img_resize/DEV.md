# img_resize 开发文档

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
img_resize/
├── src/
│   ├── main.rs            # clap builder 入口
│   ├── error.rs           # AppError 枚举
│   └── subcmd/
│       ├── mod.rs         # SubExecutor trait 定义
│       ├── r_tp.rs        # 本地图片处理逻辑
│       └── tinify_tp.rs   # TinyPNG API 集成
└── tests/                 # 单元测试
    ├── resize_tests.rs
    └── config_tests.rs
```

### 核心模块说明

**src/subcmd/r_tp.rs:**
- `re_tp()` - 图片缩放核心函数
- `convert_tp()` - 格式转换
- `exec_from_config()` - YAML 配置批量处理
- 文件遍历和过滤逻辑

**src/subcmd/tinify_tp.rs:**
- TinyPNG API 集成
- ⚠️ 注意：不要硬编码 API key

---

## 测试

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试模块
cargo test resize_tests
cargo test config_tests

# 显示测试输出
cargo test -- --nocapture
```

### 测试覆盖

**tests/resize_tests.rs:**
- ✅ 按最大像素缩放计算
- ✅ 固定宽度/高度缩放
- ✅ 纵横比保持验证
- ✅ 正方形/竖向/横向图片处理
- ✅ 无需缩放场景

**tests/config_tests.rs:**
- ✅ YAML 配置解析（有效/无效/空配置）
- ✅ 图片文件过滤（扩展名、隐藏文件）
- ✅ 大小写不敏感处理
- ✅ 嵌套路径处理

---

## 依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `image` | * | 图片处理核心库 |
| `imageproc` | * | 图像处理操作 |
| `clap` | 4 (features=["cargo"]) | CLI 框架（builder 模式） |
| `thiserror` | 1 | 错误处理 |
| `walkdir` | 2 | 目录递归遍历 |
| `infer` | 0.15 | 文件类型推断 |
| `yaml-rust` | 0.4 | YAML 配置解析 |
| `tinify-rs` | 1.4 (features=["async"]) | TinyPNG API 集成 |
| `tokio` | 1 | 异步运行时 |
| `log` / `fern` | - | 日志 |
| `rand` | 0.8 | 随机数生成 |

---

## 注意事项

- 本项目使用 clap builder 模式（旧项目），不要求迁移到 derive
- 使用 thiserror v1（与 Rust edition 2018 一致）
- TinyPNG API Key 不应硬编码，必须通过参数传入
- 处理大量图片时建议使用配置文件批量处理

---

## 构建和发布

### 本地开发

```bash
# 构建 debug 版本
cargo build

# 本地安装（macOS: /usr/local/bin）
just install_loc

# 运行
img_resize r_resize -mx 1000000 input/ output/
```

### CI/CD

通过 Jenkins Pipeline 构建发布：
- Job: `r_lit-binary-build`
- 参数: `TOOL_NAME=img_resize`
- 产物: Nexus `raw-prod/r_lit/img_resize/`

详见 [`_base_rust_cli.md`](../ci-all-in-one/_ai/rlit-dev/_base_rust_cli.md) §5 CI/CD 规范。
