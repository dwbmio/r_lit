# omniplan_covers_ding 开发文档

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
omniplan_covers_ding/
├── src/
│   ├── main.rs            # 程序入口
│   ├── error.rs           # AppError 枚举
│   ├── ctx.rs             # 应用上下文和文档模板枚举
│   └── subcmd/
│       ├── mod.rs         # 子命令模块
│       ├── convert.rs     # CSV 转 Excel 逻辑
│       └── plan_docs/
│           ├── mod.rs
│           ├── ding_require_doc.rs  # 需求文档结构
│           └── ding_task_doc.rs     # 任务文档结构
└── tests/                 # 单元测试
    └── csv_tests.rs
```

### 核心模块说明

**src/subcmd/convert.rs:**
- `read_gante_data()` - 读取 CSV 甘特图数据
- `template_xlsx_writer()` - 生成 Excel 模板
- `get_last_time_from_array()` - 时间计算工具

**src/subcmd/plan_docs/:**
- `DocRecord` trait - 文档记录接口
- `ding_require_doc.rs` - 需求文档结构（已完成）
- `ding_task_doc.rs` - 任务文档结构（⚠️ 未完成，有多处 `todo!()`）

---

## 测试

### 运行测试

```bash
# 运行所有测试
cargo test

# 运行特定测试模块
cargo test csv_tests

# 显示测试输出
cargo test -- --nocapture
```

### 测试覆盖

**tests/csv_tests.rs:**
- ✅ CSV 行解析（有效/无效格式）
- ✅ 空行处理
- ✅ 带逗号字段处理
- ✅ 日期格式转换（/ 转 -）
- ✅ 带时间的日期转换
- ✅ 数据映射（CSV 到钉钉文档）
- ✅ 时间计算（获取最晚时间）

---

## 依赖

| 依赖 | 版本 | 用途 |
|------|------|------|
| `cli-common` | path | 公共 CLI 工具库 |
| `tokio` | 1 | 异步运行时 |
| `csv` | 1.3 | CSV 文件解析 |
| `thiserror` | 2 | 错误处理 |
| `strum` / `strum_macros` | 0.27 | 枚举工具 |
| `rust_xlsxwriter` | 0.89 | Excel 文件生成 |

### Dev Dependencies

| 依赖 | 版本 | 用途 |
|------|------|------|
| `chrono` | 0.4 | 日期时间处理（测试用） |

---

## 已知问题

### 1. 任务文档功能未完成

`src/subcmd/plan_docs/ding_task_doc.rs` 中有多处 `todo!()` 未实现：

- Line 100-106: 多个字段映射未实现

当前仅支持需求文档（`require`）转换，任务文档（`task`）功能不可用。

### 2. 依赖问题

项目依赖本地路径的 `cli-common`，无法在 CI 中构建。

**当前配置：**
```toml
[dependencies.cli-common]
path = "/Users/admin/data0/private_work/crate-r-svr-api/cli-common"
```

**建议修改为：**
```toml
[dependencies.cli-common]
git = "https://github.com/dwbmio/crate-r-svr-api.git"
package = "cli-common"
rev = "指定 commit hash"
```

---

## 构建和发布

### 本地开发

```bash
# 构建 debug 版本
cargo build

# 运行
omniplan_covers_ding convert gantt.csv -o output.xlsx -t require
```

### CI/CD

⚠️ **当前无法在 CI 中构建**，需先解决 `cli-common` 依赖问题。

解决后可通过 Jenkins Pipeline 构建发布：
- Job: `r_lit-binary-build`
- 参数: `TOOL_NAME=omniplan_covers_ding`
- 产物: Nexus `raw-prod/r_lit/omniplan_covers_ding/`

详见 [`_base_rust_cli.md`](../ci-all-in-one/_ai/rlit-dev/_base_rust_cli.md) §5 CI/CD 规范。

---

## 待办事项

- [ ] 完成 `ding_task_doc.rs` 中的 `todo!()` 实现
- [ ] 修改 `cli-common` 依赖为 git 依赖
- [ ] 添加更多集成测试
- [ ] 支持更多 OmniPlan 导出格式
