# 开发期排查问题路径

当 r_lit 下某个工具出问题时（用户报 bug / 行为不符合预期 / GUI 卡顿 / 输出不对），代理在动手改代码之前，**优先按下面的顺序找证据**，而不是急着复现或猜测。

## 总原则

1. **先看日志，再动代码**。每个能落盘的工具都把"上一次运行"完整记录在边车文件里——直接读，不必重跑用户场景。
2. **日志只保留一份**。每次跑覆盖上次。所以拿到的就是用户最近一次失败/异常的状态。
3. **失败的 run 反而留下信息最完整的日志**。错误路径同样会 flush，看日志比让用户重跑更高效。

## mj_atlas（v0.3.2+）的具体路径

每次 CLI 调用、每次 GUI 自动 pack，都会在与 atlas / manifest 同目录下留一份 `<output>.log`：

| 场景                                    | 日志位置                                |
|-----------------------------------------|----------------------------------------|
| `mj_atlas pack ./sprites -o atlas`      | `./sprites/atlas.log`（或 `-d` 指定的 output_dir）|
| `mj_atlas inspect ./out/atlas.png`      | `./out/atlas.log`                      |
| `mj_atlas diff a.json b.json`           | A 文件旁边                              |
| `mj_atlas verify ./out/atlas.png`       | `./out/atlas.log`                      |
| `mj_atlas tag ./out/atlas.png ...`      | manifest 旁边                          |
| GUI 自动 pack                            | `<output_dir>/<output_name>.log`       |
| 早期 CLI 错误（参数解析等）              | `./mj_atlas.log`（cwd 兜底）           |

文件结构：

```
# mj_atlas 0.3.2 — run log
# started: <RFC3339 timestamp>
# argv:    <quoted full command line>
# subcommand: pack
# input:    ...
# output:   ...
# layout:   max_size=... spacing=... padding=... extrude=... trim=... rotate=... pot=...
# polygon:  on/off (shape=... tolerance=... max_vertices=...)
# incremental: ... format: ... quantize: ...

[<timestamp> INFO] ...
[<timestamp> DEBUG] ...   ← DEBUG 行只在文件里，不到 stdout
[<timestamp> WARN] ...
[<timestamp> ERROR] ...
```

**代理动作清单**：

1. 用户描述了 mj_atlas 问题（"删除没刷新"、"打包结果不对"、"crash"）→ **先问用户要 `<atlas>.log` 文件内容**，或者从用户提供的工程路径自己读。
2. 检查 `# argv` 行：用户实际跑了什么参数？跟 `# subcommand` 摘要对比，是不是预期的。
3. 检查 `# layout` / `# polygon` / `# incremental` 摘要：选项是否符合用户描述的场景。
4. 扫底部最后 20 行：找 `ERROR`、`WARN`、`partial: ... bail to full`、`incremental: ... full repack` 这类关键信号。
5. DEBUG 行透露大量内部决策（partial repack 的 fit 失败原因、增量 diff 分类、manifest 加载失败原因）——99% 的问题靠这些就能定位根因。
6. 只有日志看完仍未明确根因时，才要求用户复现或 `--force` 重跑。

## 通用约定（向其他子工具扩散）

只要是"短时运行的 CLI 工具"（即 `r_lit` 仓库定位），新增功能时应当：

| 约定                                  | 实现要点                                                |
|---------------------------------------|--------------------------------------------------------|
| 在主输出物旁边留一份 `<output>.log`   | 对长时运行的服务/守护进程**不适用**——但本仓库不收容这些 |
| 单文件覆盖式，不做 rotation           | 用户要更长历史就让他们自己 tee/copy；工具不操心          |
| 失败路径也要 flush                    | 这才是日志最有价值的场景                                 |
| 包含完整 argv + 解析后的选项摘要       | 头部 `# ` 注释行；让 review 时能不依赖 `--verbose` 复跑   |
| DEBUG 进文件不进 stdout                | 文件用 `LevelFilter::Debug`，stdout 沿用 `Info`         |

参考实现：`mj_atlas/src/runlog.rs`（自定义 `log::Log` 双 sink + `flush(path, &header)`）。新工具复用这一套是最低成本的做法。

## 反模式

- ❌ 用户报问题，第一反应是"能不能本地复现一下"。**先看日志**。
- ❌ 把日志路径设计成跨 run 累加（`.log` 越长越好）。会污染 git status、影响 grep、且大部分历史价值低。
- ❌ 把日志只发到 stdout、依赖用户重定向。用户没保留 stdout 时一切都靠重跑。
- ❌ 失败时只输出一行错误就退出，不写日志。最重要的场景反而没保留信息。
- ❌ 为了"日志干净"而在错误路径 panic / `process::exit` 跳过 flush。代码里 panic 路径要么也 flush，要么用 `Drop` guard 覆盖。
