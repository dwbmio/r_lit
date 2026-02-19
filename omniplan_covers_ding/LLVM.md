# omniplan_covers_ding - OmniPlan 转钉钉文档工具

将 OmniPlan 导出的 CSV 甘特图转换为钉钉项目管理可导入的 Excel 格式，支持需求文档和任务文档两种类型。

---

## 快速开始

```bash
# 转换为需求文档
omniplan_covers_ding convert gantt.csv -o output.xlsx -t require

# 转换为任务文档（带父任务和迭代）
omniplan_covers_ding convert gantt.csv -o tasks.xlsx -t task -p "父任务ID" -i "迭代1"
```

---

## 命令参考

### `convert` - CSV 转 Excel

将 OmniPlan 导出的 CSV 甘特图转换为钉钉导入格式的 Excel 文件。

```bash
omniplan_covers_ding convert <CSV_PATH> -o <OUTPUT> -t <TYPE> [OPTIONS]
```

| 参数 | 必需 | 说明 |
|------|------|------|
| `<CSV_PATH>` | 是 | OmniPlan 导出的 CSV 文件路径 |
| `-o, --output <FILE>` | 是 | 输出 Excel 文件路径 |
| `-t, --type <TYPE>` | 是 | 文档类型：`require`（需求）或 `task`（任务） |
| `-p, --parent <ID>` | 否 | 父任务 ID |
| `-i, --iteration <NAME>` | 否 | 迭代归属名称 |

---

## 典型场景

### 场景1：转换为需求文档

从 OmniPlan 导出需求列表并转换：

```bash
omniplan_covers_ding convert requirements.csv -o 需求导入.xlsx -t require -i "2024Q1迭代"
```

### 场景2：转换为任务文档（带父任务）

将子任务关联到父需求：

```bash
omniplan_covers_ding convert tasks.csv -o 任务导入.xlsx -t task -p "REQ-001" -i "Sprint 1"
```

### 场景3：批量导入项目计划

1. 在 OmniPlan 中规划项目
2. 导出为 CSV 格式
3. 转换为钉钉格式
4. 在钉钉项目管理中批量导入

---

## 数据格式说明

### OmniPlan CSV 输入格式

OmniPlan 导出的 CSV 应包含以下列：

| 列名 | 说明 | 示例 |
|------|------|------|
| 任务名称 | 任务/需求标题 | "用户登录功能" |
| 开始日期 | 格式：YYYY/MM/DD | "2024/01/15" |
| 结束日期 | 格式：YYYY/MM/DD | "2024/01/31" |
| 工期 | 天数 | "5天" |
| 负责人 | 可选 | "张三" |

### 钉钉 Excel 输出格式

生成的 Excel 包含以下列：

| 列名 | 说明 |
|------|------|
| 标题 | 从 CSV 的任务名称映射 |
| 开始时间 | 格式转换为 YYYY-MM-DD |
| 结束时间 | 格式转换为 YYYY-MM-DD |
| 父任务 ID | 从参数 `-p` 指定 |
| 迭代归属 | 从参数 `-i` 指定 |
| 限制条件 | 可选字段 |

---

## 日期格式转换

工具会自动将日期格式从 OmniPlan 的 `/` 格式转换为钉钉的 `-` 格式：

| OmniPlan 格式 | 钉钉格式 |
|--------------|---------|
| 2024/01/15 | 2024-01-15 |
| 2024/12/31 | 2024-12-31 |

---

## 文档类型说明

### require（需求文档）

用于导入需求列表，通常用于：
- 产品需求规划
- 功能模块划分
- 里程碑管理

### task（任务文档）

用于导入任务列表，通常用于：
- 开发任务分解
- 子任务关联
- Sprint 任务规划

⚠️ **注意：** 任务文档功能当前未完全实现。

---

## 使用建议

1. **先在 OmniPlan 中规划好项目结构**
   - 合理划分任务层级
   - 设置准确的时间估算

2. **导出时注意格式**
   - 确保日期格式为 YYYY/MM/DD
   - 检查必需字段是否完整

3. **转换后验证数据**
   - 在 Excel 中检查转换结果
   - 确认日期格式正确
   - 验证父子关系

4. **分批导入钉钉**
   - 先导入需求文档
   - 再导入关联的任务文档
