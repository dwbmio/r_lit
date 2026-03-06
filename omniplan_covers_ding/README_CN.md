# omniplan_covers_ding

> 将 OmniPlan CSV 导出文件转换为钉钉兼容的 Excel 文档。

## 功能

读取 OmniPlan 导出的 CSV 文件，转换为结构化 Excel 文件，可直接导入钉钉项目管理（任务或需求）。

## 使用

```bash
omniplan_covers_ding convert <csv-file> <doc-type> [-p <parent>] [-t <liter>] [-l <limit>]
```

### 参数

- `<csv-file>`: 输入 CSV 文件路径
- `<doc-type>`: 输出文档类型 — `task`（任务）或 `require`（需求）
- `-p, --parent <value>`: 父任务值
- `-t, --liter <value>`: 所属迭代
- `-l, --limit <value>`: 过滤条件，`k=v` 格式

### 示例

```bash
# 转为任务格式
omniplan_covers_ding convert plan.csv task -p "Sprint 1"

# 转为需求格式
omniplan_covers_ding convert plan.csv require -t "Team A"
```

## 构建

```bash
cargo build --release
```

**注意：** 此工具依赖外部路径的 `cli-common` 库，无法在其他机器上直接构建。

## License

See LICENSE file.
