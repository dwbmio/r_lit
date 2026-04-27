# sonargrid 侧 — `texture.gen` Worker 实现路线图

**文档归属.** 这份文档住在 Maquette 仓里，因为 Maquette 是
Rustyme `texture.gen` 任务的**唯一消费方**（目前）。协议就是
Maquette 定的，所以未来协议升级也会先改这里；sonargrid 侧
worker 实现应**按本文档和 `rustyme.md` 照做**，不要单方面
调整 payload 形状。

**读者.** sonargrid 项目里要写 worker 的人（或 agent）。

**起点.** 你已经有 Rustyme 的全套基础设施：actor 调度、
Redis processing queue、重试退避、DLQ、revoke、Beat、Canvas、
rate limit。本路线图只解决一件事 —— **在这套基础设施里挂一个
能响应 `texture.gen` 任务的 PhaseHook**，并把它喂给 Fal.ai（或
别的生图 API）。

---

## 0. 先读这俩文档（30 min）

1. `maquette/docs/texture/rustyme.md` —— Maquette 作为**生产者**
   的视角：envelope 字段、结果形状、超时与 revoke 语义。**这是
   协议，权威**。
2. 你自己项目的 `rustyme/examples/echo_consumer.rs` —— worker
   最小骨架，直接改它就能跑。

下面的"验收口径"一节列了六条命令，跑通就算结项。

---

## 1. 分阶段交付（建议顺序）

### Stage 1 · Echo worker（半天，解锁 Maquette 侧联调）

**目标.** Maquette 的 `#TEX-B` 清单能端到端跑完。**不调任何
外部 API**，worker 只负责：读到 `texture.gen` 任务 → 合成一张
固定占位 PNG → base64 编码 → 返回 `{"png_b64": "..."}`。

Maquette 这头完全不关心你返回的是真 AI 图还是一个灰色方块 —
只要 PNG 合法，管道就通了。先把这一段做了，后面换 Fal 的
工作量就缩成"改 on_process 里的 20 行"。

**实现要点：**

* `TextureGenHook` 实现 `rustyme_core::hook::PhaseHook`，只重写
  `on_process` 一个方法。
* 入参：`envelope.kwargs` 里必有 `prompt / seed / width / height /
  model / cache_key`（但 Stage 1 只需要读 `width / height` 合成图，
  其余字段打日志或忽略都行）。
* 合成 PNG 用 `image` crate 或你已有的 `png` 依赖：填一张
  纯色或带文字的占位图，再 `STANDARD` base64 编码。
* 返回：`serde_json::json!({"png_b64": b64})` —— 框架会自动
  包成 `{task_id, status: "SUCCESS", result, metadata}` 写回
  `result_key`，这部分是 Rustyme worker runtime 既有行为。
* 注册：`QueueEntry { name: "texgen", queue_key:
  "rustyme:texgen:queue", result_key: Some("rustyme:texgen:result"),
  concurrency: 2, task_timeout_secs: 30, ... }`；
  `HookRegistry::register("texgen-group", Arc::new(TextureGenHook))`。

**验收.**

```sh
# 起 worker（sonargrid 侧）
cargo run -p rustyme --example texgen_echo_consumer

# Maquette 侧（r_lit）
export MAQUETTE_RUSTYME_REDIS_URL=redis://localhost:6379/0
export MAQUETTE_RUSTYME_ADMIN_URL=http://localhost:12121
cargo run --bin maquette-cli -- texture gen --provider rustyme \
    --prompt "hello from rustyme" --seed 1 --no-cache -o /tmp/r.png

file /tmp/r.png          # 应显示 PNG image data
ls -lh /tmp/r.png         # 非 0 字节
```

### Stage 2 · Fal.ai FLUX schnell 真接（1 天）

**目标.** `on_process` 里调 Fal HTTP，把 prompt 真变成图。

**实现要点：**

* 依赖：`reqwest = { version = "0.12", features = ["rustls-tls", "json"] }`
  或你们项目里现成的 HTTP client。
* 配置（env）：
  * `FAL_KEY` — 必填，Fal API key。
  * `FAL_MODEL` — 默认 `fal-ai/flux/schnell`；允许 per-task 覆盖
    （看 `envelope.kwargs["model"]` 是否非空）。
* 调用：Fal 的 schnell 接口返回的是图片 URL（不是 bytes），所以
  `on_process` 内部要走两步：
  1. `POST https://fal.run/fal-ai/flux/schnell` —— body 里带
     `prompt / seed / image_size` 之类。
  2. 拿到 `response.images[0].url` → 再 `GET` 一次拿 PNG bytes。
  3. base64 编码返回。
* Seed 对齐：Maquette 的磁盘缓存和 determinism 契约要求 "同
  prompt + 同 seed + 同 model = 同 bytes"。FLUX schnell 确实
  支持 `seed` 参数 —— 一定要把 `envelope.kwargs["seed"]` 传
  过去，别偷懒用随机。Fal 侧偶尔会不保证严格 bit-identical，
  只要视觉一致即可接受，但**不要主动再加一层随机**。
* 错误映射：**Fal 返回 4xx/5xx 要 `return Err(SonarError::Hook(...))`**，
  不要 `panic!`；框架会按 `max_retries` 自动重试，超了会进 DLQ
  并最终让 Maquette 收到 `status: "FAILURE"`。
* 超时：`reqwest` 的 `.timeout()` 设成 20s；Maquette 默认
  `RESULT_TIMEOUT_SECS=60` 给了你 3× 的裕量。

**额外要做的（否则后面会踩坑）：**

* **本地副缓存（可选但强烈建议）.** worker 进程内用 `moka` 或
  简单的 `HashMap<cache_key, TextureBytes>` 把"刚出的图"留一会儿。
  原因：Maquette 端已经有磁盘缓存，所以"同请求重复打 worker"通常
  不会发生；但**并发两个 Maquette 实例同时 LPUSH 相同 cache_key
  的请求**时，worker 侧加一层 in-memory LRU 能把外部 API 账单
  打五折。
* **费用打点.** `on_complete` 里 `tracing::info!(cost_usd=0.003,
  elapsed_ms=?, model=?, ...)` 发到你们的 Prometheus/Grafana。
  `#TEX-B` 跑通后我们会开始批量生成，事先能看到每日成本很重要。

**验收.**

```sh
export FAL_KEY=sk-...
cargo run -p rustyme --example texgen_fal_consumer

# Maquette 侧
cargo run --bin maquette-cli -- texture gen --provider rustyme \
    --prompt "isometric grass block, low-poly, seamless" \
    --seed 42 --no-cache -o /tmp/g.png

open /tmp/g.png   # 应看到真 AI 草地贴图
```

第二次重复跑（不带 `--no-cache`）应**秒返回**，因为命中
Maquette 本地磁盘缓存 —— 不会再进 Rustyme 队列。

### Stage 3 · 生产健壮性（2-3 天）

这一阶段开始必须，否则走不到 v1.0。

* **Rate limit.** 配置 `QUEUE_0_RATE_LIMIT=30/m`（Fal 的
  schnell 免费额度在这个量级）。Rustyme 已经有 Redis Lua
  限流，直接配 env 即可。
* **指数退避.** `retry_backoff_secs=5, retry_backoff_max_secs=60`，
  Fal 偶发 429 时让重试自己摊平。
* **观测性.** 确认以下日志能打出来（字段全了就行，格式随意）：

  ```
  task_id, cache_key, model, prompt_head (前 60 字符), elapsed_ms,
  upstream_status, cost_usd, png_bytes
  ```

  这对 Maquette 侧判"是我们发得不对"还是"worker/API 出问题"
  至关重要。Maquette 的 `log::info!` 里已经打了 `task_id` +
  `bytes` + 耗时，两边对齐后 `grep task_id` 就能串。
* **Alert.** 对接你们现成的 n8n-alert 告警链：
  * `upstream_status >= 500` 连续 5 次 → alert。
  * `DLQ` 深度 > 20 → alert。
  * 我这边不关心具体 webhook，只要 alert 触发时能推个
    task_id/cache_key 回来即可。
* **Revoke 的语义确认.** Rustyme 框架自己处理"task 在进入
  on_process 前被 revoke 掉 → 直接 `TaskStatus::Revoked`"的情况，
  这部分不用你额外做。风险点只剩"worker 正在调 Fal 时 revoke
  到达"—— 这一场景可以先不管，Maquette 侧 timeout 后返回错误，
  worker 把 API 钱花出去了也就花出去了，下一次不会重复花。
  **v1.1 再考虑** `tokio::select!` 监听撤销信号提前中止 Fal
  请求；现在做反而会把代码搞乱。

### Stage 3.5 · Canvas group fan-out 保活（Maquette D-1 的强依赖）

> **2026-04-24 更新：** 原本 Stage 4 里的 "Canvas group 批量生成" 从
> "可选进阶"**升级成 Stage 3.5**，因为 Maquette v0.10 **D-1**
> 设计拍板后确定会用到 —— 用户打一句 "这是什么" → Maquette 对每个
> 非空 palette slot 扇出一个 `texture.gen` 子任务，挂同一个
> `group_id`，等整组全部完成后才让 GUI 刷新贴图。

这一阶段 worker 端**理论上一行代码都不用改** —— Rustyme runtime
的 worker loop 看到 `envelope.group_id` 非空时会自己 `HINCRBY
counter.done`、`HSET results`、到达 `total` 时自动投递
`chord_callback`。我们要做的是**验证这条路径在 texgen worker 上
确实跑通了**，不要 Stage 2 上线了才发现 group semantics 有坑。

**专项验收（在 Stage 2 Fal 接入之后做一次）：**

* 手工 Redis 命令并发 LPUSH 3 条 `texture.gen` envelope，字段：
  * 全都 `group_id="test-grp-1"`，
  * 第 1 条带 `chord_callback={"task":"texgen.done","queue_key":"rustyme:texgen:result"}`，
    其余两条**不带 chord_callback**（按 Canvas 协议任一条带上即可）。
* 之前手动 `HSET rustyme:group:test-grp-1:counter total 3`。
* 观察：
  * Worker 日志按顺序处理 3 条；
  * `HGETALL rustyme:group:test-grp-1:results` 有 3 个 key，value
    是每张 `{png_b64:...}`；
  * `rustyme:texgen:result` 里**多了一条 callback envelope**（task
    = `texgen.done`，kwargs 里有 `results` 数组 + `group_id` +
    `total=3`）。
* **这一条 callback envelope 是 Maquette 的锚点** —— D-1 GUI 靠它
  知道 "这批 3 张都好了，可以一次性刷进调色板"。所以这个环节不要
  偷工，哪怕 Stage 1 就先跑通一次。

如果这里不 work，Maquette D-1 会被迫降级到"一个 slot 一个 slot 
串行等 BRPOP"，丢掉并发优势，用户体验立马从"点一下几秒后全刷新"
变成"每个色等一会儿、界面抽动"。

### Stage 4 · 可选进阶（不阻塞 Maquette v1.0）

下面这些不进 v1.0，提前写个 TODO 留坑即可：

* **多优先级队列.** Maquette GUI 点 Generate 时可能想走 `high`
  优先级。目前 hardcode `normal`；等 D-2 单 slot regenerate
  有了交互量级再改。
* **Replicate / 自部署 SDXL 多 provider.** `envelope.kwargs.model`
  字段已经是分路由路标。写两个 PhaseHook 注册到两个不同的
  group，或者在单 hook 里 match model 前缀分派。
* **物件存储 offload.** 当支持 512²/1024² 时 inline base64 会
  难受，改成 worker 上传到 S3/MinIO 返回 URL。这时候需要
  Maquette 侧 bump task name 到 `texture.gen.v2` 并在 result
  schema 里加 `url / sha256` 字段 —— 这是**协议断代升级**，
  提前打招呼，两边联合发版。

---

## 2. 代码骨架参考

下面这段是 Stage 1 Echo worker 的最小骨架，照 `echo_consumer.rs`
改就能起来。只给示意，具体放你们 `rustyme/examples/` 还是单独
新 crate 由你们决定。

```rust
// rustyme/examples/texgen_echo_consumer.rs
use std::sync::Arc;
use async_trait::async_trait;
use base64::Engine;
use image::{Rgba, RgbaImage};
use rustyme_core::error::SonarError;
use rustyme_core::hook::PhaseHook;
use rustyme_core::protocol::TaskEnvelope;
use serde_json::{json, Value};

pub struct TextureGenEchoHook;

#[async_trait(?Send)]
impl PhaseHook for TextureGenEchoHook {
    fn name(&self) -> &str { "texgen-echo" }
    fn kind(&self) -> &str { "rust" }

    async fn on_process(&self, envelope: &TaskEnvelope) -> Result<Value, SonarError> {
        let kw = &envelope.kwargs;
        let w = kw.get("width").and_then(|v| v.as_u64()).unwrap_or(128) as u32;
        let h = kw.get("height").and_then(|v| v.as_u64()).unwrap_or(128) as u32;
        let prompt = kw.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

        // Prompt 头 4 字节决定占位色，Stage 1 就够用了
        let seed_color = prompt.bytes().take(3).collect::<Vec<_>>();
        let (r, g, b) = (
            *seed_color.first().unwrap_or(&128),
            *seed_color.get(1).unwrap_or(&128),
            *seed_color.get(2).unwrap_or(&128),
        );

        let mut img = RgbaImage::new(w, h);
        for p in img.pixels_mut() {
            *p = Rgba([r, g, b, 255]);
        }
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| SonarError::Hook(format!("png encode: {e}")))?;

        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);
        Ok(json!({ "png_b64": b64 }))
    }
}

// main 部分跟 echo_consumer.rs 一模一样，改三处：
// 1. name / queue_key / result_key → texgen
// 2. group 名 → "texgen-group"
// 3. register(..., Arc::new(TextureGenEchoHook))
```

Stage 2 的 `TextureGenFalHook` 区别只在 `on_process` 内部：
构造 reqwest 请求 → 下载返回 URL → base64 编码；其余 Rustyme
框架全部不变。

---

## 3. Lua hook 快速 PoC（可选、3 小时出 demo）

如果你们想先快速摸底 Fal API 行为再决定 Rust 实现，Rustyme
的 `lua` feature 直接能跑脚本。参照 `docs/healthcheck-dogfood.md`
的写法，用 Lua `http.post` + `json.encode` 搞 Fal 调用，
返回 `{png_b64=...}`。这套是一次性工具，别上生产，原因：

* Lua 里 base64 编码大数组很慢（128×128 PNG ≈ 50 KB 尚可，
  512² 起就憋得住）。
* 密钥管理、retry 策略、并发限流都不如 Rust 实现干净。

想跑 PoC 时来说一句，我给一份对齐协议的 Lua 脚本。

---

## 4. 环境与配置清单

（大部分 sonargrid 这边已经有，这里列一下和 texgen 相关的）

| 变量 | 值示例 | 说明 |
|---|---|---|
| `QUEUE_0_NAME` | `texgen` | 逻辑队列名，**必须与 Maquette `MAQUETTE_RUSTYME_QUEUE_KEY` 去掉 `rustyme:` 前缀 + `:queue` 后缀对齐**。实际 Redis key 由 `QUEUE_0_KEY` 决定。 |
| `QUEUE_0_KEY` | `rustyme:texgen:queue` | LPUSH/BRPOPLPUSH 目标。 |
| `QUEUE_0_RESULT_KEY` | `rustyme:texgen:result` | 成功/失败结果写入此 list；**必填**，否则 Maquette 永远收不到回写。 |
| `QUEUE_0_GROUP` | `texgen-group` | 你在 `HookRegistry` 里注册的 group 名。 |
| `QUEUE_0_CONCURRENCY` | `2` (Stage 1) / `4` (Stage 2+) | Fal 并发能力通常到 5 左右，先保守。 |
| `QUEUE_0_TASK_TIMEOUT` | `60` | ≥ Maquette `RESULT_TIMEOUT_SECS`，否则 worker 侧强杀但 Maquette 还在等。 |
| `QUEUE_0_RATE_LIMIT` | `30/m` (Stage 3+) | Fal schnell 免费额度量级。 |
| `QUEUE_0_RETRY_BACKOFF` | `5` | 退避基数秒。 |
| `QUEUE_0_RETRY_BACKOFF_MAX` | `60` | 退避上限。 |
| `FAL_KEY` | `sk-xxx` (Stage 2+) | Fal API key；Stage 1 不需要。 |
| `FAL_MODEL` | `fal-ai/flux/schnell` | 默认 model；被 `kwargs.model` 覆盖。 |

---

## 5. 联合验收清单（双方都勾上就算结）

**Maquette 侧（我负责验）：**

* `cargo run --bin maquette-cli -- texture gen --provider rustyme
  --prompt ... --no-cache -o /tmp/x.png` 返回 0，`/tmp/x.png`
  是合法 PNG。
* 重复同一 prompt（不带 `--no-cache`）秒返回 + 日志 `cache hit`。
* 故意用短 `RESULT_TIMEOUT_SECS=1`：超时后命令以非零退出，日志
  有 `revoke(<task_id>)`；Rustyme admin UI 看到该任务状态为
  `REVOKED` 或 `DEAD`。
* `maquette-cli texture purge texgen --admin-url ...` 成功清空
  pending。

**sonargrid 侧（你负责验）：**

* `texgen` 队列里有 task 时，worker 日志打 `task_id, cache_key,
  prompt_head, elapsed_ms`。
* 故意让 Fal 返回 5xx（Stage 2+）：连续 N 次后任务进 DLQ，
  Maquette 侧收到 `status=FAILURE` 且 `error` 非空字符串。
* `curl http://localhost:12121/api/admin/overview` 能看到
  `texgen` 队列的 processing / done / failed 计数。
* Prometheus 有 `rustyme_task_duration_seconds{queue="texgen"}`
  的直方图样本。

---

## 6. 两边的联调频率

**Stage 1 完成前.** 每天对齐一次：你给 Redis 连接串，我跑
Maquette 侧 CLI，有问题现场 grep 日志。

**Stage 2 测试期间.** 建议开一个共享 Redis DB（非 prod），
双方都指过去，改好 worker 我立刻用 `maquette-cli texture
gen --provider rustyme --no-cache` 压几轮，省得你自己写
producer 测试脚本。

**Stage 3 上线后.** 只要 Prometheus 面板有告警我这边就跟进；
日常出图没问题时两边解耦。

---

## 7. 非目标（明确不要做）

以下事项**不在 texgen worker 的责任范围内**，如果 sonargrid 侧
有人提出来请拒绝：

* **PNG 优化（pngcrush、oxipng）.** Maquette 自己不 care PNG
  文件大小，也不会重压缩；加压缩会拖慢 `elapsed_ms`。
* **图片后处理（锐化、尺寸缩放）.** `width / height` 应该原样
  传给 Fal，不在 worker 本地缩放。Maquette 拿到的 PNG 像素尺寸
  必须和请求一致，否则 UV 贴图会错位。
* **Prompt rewriting / 模板注入.** Maquette 发什么 prompt，就用
  什么 prompt。加前缀（"isometric low-poly style, "）是 Maquette
  GUI / 调色板模板的职责，v0.10 D 之后会在生产者侧处理。
* **多语言翻译.** 同上。

---

## 8. 参考链接

* 协议权威：`maquette/docs/texture/rustyme.md`
* 生产者实现：`maquette/src/texgen.rs` 的 `pub mod rustyme`
* PhaseHook trait：`sonargrid/rustyme-core/src/hook/mod.rs`
* 最小 worker 骨架：`sonargrid/rustyme/examples/echo_consumer.rs`
* Fal.ai FLUX schnell 文档：<https://fal.ai/models/fal-ai/flux/schnell>
