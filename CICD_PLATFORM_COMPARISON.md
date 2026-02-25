# CI/CD 平台价格横向对比（2026）

## 执行摘要

对于你的项目（7 个平台，每月 2 次发版）：

| 平台 | 月成本 | 年成本 | 免费额度 | 推荐度 |
|------|--------|--------|---------|--------|
| **GitHub Actions** | **$1.28** | **$15.36** | 2,000 分钟 | ⭐⭐⭐⭐⭐ |
| CircleCI | $15+ | $180+ | 6,000 credits | ⭐⭐⭐ |
| GitLab CI | $0-29 | $0-348 | 400 分钟 | ⭐⭐⭐⭐ |
| Travis CI | 已不推荐 | - | 有限 | ⭐ |
| Buildkite | $30+ | $360+ | 无 | ⭐⭐ |

**结论：GitHub Actions 是最佳选择！**

---

## 详细对比

### 1. GitHub Actions（推荐）✅

#### 定价（2026）

**免费额度：**
- 2,000 分钟/月（Linux 倍率）
- 公开仓库：无限制

**付费价格：**
- Linux: $0.008/分钟 + $0.002 平台费 = $0.010/分钟
- macOS: $0.080/分钟 + $0.002 平台费 = $0.082/分钟
- Windows: $0.016/分钟 + $0.002 平台费 = $0.018/分钟

#### 你的项目成本

**单次发版（7 平台）：**
- Linux musl (3 平台): 6 分钟 × $0.010 = $0.06
- macOS (2 平台): 6 分钟 × $0.082 = $0.492
- Windows (2 平台): 7 分钟 × $0.010 = $0.07
- Release: 2 分钟 × $0.010 = $0.02
- **总计：$0.64/次**

**月成本（2 次发版）：** $1.28
**年成本：** $15.36

**免费额度使用：**
- 单次消耗：75 分钟（Linux 等效）
- 可免费发版：26 次/月

#### 优势

✅ **与 GitHub 深度集成**
✅ **免费额度充足**（2,000 分钟）
✅ **价格透明**
✅ **支持所有主流平台**
✅ **社区支持好**
✅ **配置简单**

#### 劣势

❌ macOS runner 较贵（10x 倍率）
❌ 并发限制（免费账户）

---

### 2. CircleCI

#### 定价（2026）

**免费计划：**
- 6,000 credits/月
- Linux: 10 credits/分钟
- macOS: 50 credits/分钟

**Performance 计划：**
- $15/月起（25,000 credits）
- 额外 credits: $0.0006/credit

#### 你的项目成本

**单次发版 credits 消耗：**
- Linux musl: 6 分钟 × 10 = 60 credits
- macOS: 6 分钟 × 50 = 300 credits
- Windows: 7 分钟 × 20 = 140 credits
- **总计：~500 credits**

**月成本（2 次发版）：**
- 消耗：1,000 credits
- 免费额度：6,000 credits
- **成本：$0（免费额度内）**

**但是：**
- 如果超过 6,000 credits/月
- 需要购买 Performance 计划：$15/月起
- 25,000 credits 可支持 50 次发版

**年成本：** $0-180（取决于使用量）

#### 优势

✅ 免费额度较大（6,000 credits）
✅ Docker 支持好
✅ 配置灵活

#### 劣势

❌ Credits 计算复杂
❌ macOS 消耗大（50 credits/分钟）
❌ 超过免费额度后成本跳跃大（$15/月起）
❌ 与 GitHub 集成不如 Actions

---

### 3. GitLab CI

#### 定价（2026）

**免费计划：**
- 400 分钟/月（私有项目）
- 无限制（公开项目）

**Premium 计划：**
- $29/用户/月
- 10,000 分钟/月

**Ultimate 计划：**
- $99/用户/月
- 50,000 分钟/月

#### 你的项目成本

**单次发版消耗：**
- 约 21 分钟（总时间）

**月成本（2 次发版）：**
- 消耗：42 分钟
- 免费额度：400 分钟
- **成本：$0（免费额度内）**

**如果需要更多：**
- Premium: $29/月（10,000 分钟）
- **年成本：** $0-348

#### 优势

✅ 公开项目无限制
✅ 完整的 DevOps 平台
✅ 自托管选项

#### 劣势

❌ 免费额度小（400 分钟）
❌ 付费计划贵（$29/用户/月起）
❌ 需要迁移到 GitLab
❌ macOS runner 支持有限

---

### 4. Travis CI

#### 现状

**Travis CI 已经不再推荐：**
- 2020 年后大幅涨价
- 免费计划大幅削减
- 社区支持减少
- 许多项目已迁移

**定价：**
- 免费计划：非常有限
- 付费计划：$69/月起

#### 结论

❌ **不推荐使用**

---

### 5. Buildkite

#### 定价（2026）

**模式：** 按 agent 数量收费，自己提供硬件

**定价：**
- $15-30/agent/月
- 需要自己维护 runner 硬件

#### 你的项目成本

**需要的 agents：**
- Linux: 1 agent
- macOS: 1 agent（需要 Mac mini）
- Windows: 1 agent

**月成本：**
- Buildkite: 3 × $30 = $90/月
- 硬件成本：Mac mini ~$600（一次性）
- **年成本：** $1,080 + 硬件

#### 优势

✅ 完全控制硬件
✅ 适合大规模使用

#### 劣势

❌ 需要维护硬件
❌ 初始成本高
❌ 配置复杂
❌ 不适合小项目

---

## 综合对比表

### 价格对比（你的项目）

| 平台 | 免费额度 | 月成本 | 年成本 | 备注 |
|------|---------|--------|--------|------|
| **GitHub Actions** | 2,000 分钟 | **$1.28** | **$15.36** | 最佳选择 |
| CircleCI | 6,000 credits | $0-15 | $0-180 | 免费额度大但复杂 |
| GitLab CI | 400 分钟 | $0-29 | $0-348 | 需迁移到 GitLab |
| Travis CI | 很少 | $69+ | $828+ | 不推荐 |
| Buildkite | 无 | $90+ | $1,080+ | 需自己硬件 |

### 功能对比

| 功能 | GitHub Actions | CircleCI | GitLab CI | Travis CI | Buildkite |
|------|---------------|----------|-----------|-----------|-----------|
| GitHub 集成 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐ |
| macOS 支持 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐⭐ |
| 配置简单 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ |
| 价格透明 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ |
| 免费额度 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐⭐ | ⭐⭐ | ⭐ | ⭐ |
| 社区支持 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ⭐⭐ | ⭐⭐⭐ |

---

## 特殊场景分析

### 场景 1：公开项目

**最佳选择：GitHub Actions 或 GitLab CI**
- GitHub Actions：2,000 分钟/月
- GitLab CI：无限制（公开项目）

**成本：** 完全免费

### 场景 2：高频发版（每周 5 次）

**月消耗：**
- GitHub Actions: 20 × 75 = 1,500 分钟
- 成本：$0（免费额度内）

**CircleCI:**
- 20 × 500 = 10,000 credits
- 需要 Performance 计划：$15/月

**结论：** GitHub Actions 仍然最优

### 场景 3：仅 Linux 构建

**月消耗：**
- GitHub Actions: 2 × 8 = 16 分钟
- 成本：$0

**所有平台都免费！**

### 场景 4：大规模企业（每天多次发版）

**月消耗：**
- 60 次 × 75 分钟 = 4,500 分钟

**GitHub Actions:**
- 超出：2,500 分钟
- 成本：~$25/月

**CircleCI:**
- 需要更大计划
- 成本：$50-100/月

**Buildkite:**
- 固定成本：$90/月
- 可能更划算

**结论：** 大规模使用考虑 Buildkite

---

## 推荐决策树

```
你的项目是公开的吗？
├─ 是 → GitHub Actions（免费）
└─ 否 → 继续

每月发版次数？
├─ < 20 次 → GitHub Actions（$0-15/月）
├─ 20-50 次 → GitHub Actions（$15-40/月）
└─ > 50 次 → 考虑 Buildkite

需要 macOS 构建吗？
├─ 是 → GitHub Actions（最好的 macOS 支持）
└─ 否 → GitHub Actions 或 CircleCI

已经在使用 GitLab？
├─ 是 → GitLab CI
└─ 否 → GitHub Actions
```

---

## 最终建议

### 对于你的项目

**强烈推荐：GitHub Actions** ⭐⭐⭐⭐⭐

**理由：**

1. **价格最优：** $15.36/年
2. **免费额度充足：** 26 次/月
3. **GitHub 原生集成：** 无缝体验
4. **配置最简单：** 已经完成
5. **社区支持最好：** 大量资源
6. **macOS 支持最好：** 官方 runner
7. **价格透明：** 无隐藏费用

**替代方案：**

- **CircleCI：** 如果需要更大免费额度（6,000 credits）
- **GitLab CI：** 如果已经使用 GitLab
- **Buildkite：** 如果是大型企业且需要完全控制

### 成本对比总结

对于你的配置（7 平台，2 次/月）：

| 排名 | 平台 | 年成本 | 性价比 |
|------|------|--------|--------|
| 🥇 | **GitHub Actions** | **$15.36** | ⭐⭐⭐⭐⭐ |
| 🥈 | CircleCI | $0-180 | ⭐⭐⭐⭐ |
| 🥉 | GitLab CI | $0-348 | ⭐⭐⭐ |
| 4 | Buildkite | $1,080+ | ⭐⭐ |
| 5 | Travis CI | $828+ | ⭐ |

---

## Sources

- [GitHub Actions Pricing Changes](https://resources.github.com/actions/2026-pricing-changes-for-github-actions)
- [CircleCI Pricing](https://www.getmonetizely.com/articles/circleci-vs-github-actions-which-cicd-pipeline-tool-offers-better-value-for-your-devops-team)
- [CI/CD Cost Calculator](https://calculator-cloud.com/other/cicd-cost/)
- [GitLab Pricing](https://checkthat.ai/brands/gitlab/pricing)
- [Buildkite Alternatives](https://betterstack.com/community/comparisons/buildkite-alternatives/)
- [GitHub Actions vs CircleCI](https://www.peerspot.com/products/comparisons/buildkite_vs_github-actions)

---

**结论：坚持使用 GitHub Actions，这是最佳选择！** 🎯
