# mini_reg

自家公司小程序的半自动注册辅助工具，覆盖微信、抖音、QQ、支付宝。
命令、SQLite 状态库、浏览器 HTTP/WebSocket 连接和填表逻辑均内置在 Rust
可执行文件中。运行无需 Python、Node.js、Playwright、外置数据库或常驻服务。

交互部分需要已存在的、独立的 `gamesci-lite.com` 企业 Chrome，并已开启本机调试。
工具不下载、启动、改配或替换浏览器。个人配置和混用个人配置的数据目录会被拒绝。

## 当前范围

- 整批导入 TOML 清单；重复导入保留进度。
- 跨运行保存任务、断点和浏览器绑定。
- 在已核实的企业浏览器中打开各平台官方入口。
- 预览或填写唯一匹配的、可见的名称与简介文本框。
- 不覆盖已有不同内容；输入框不唯一时停止。
- 写入中断时保留 `needs_review`，不登记成功。
- 人工确认后登记应用编号；导出结果不包含登录资料。

**尚未通过真实后台表单验收。** 当前使用明确的字段标签或手动配置的 CSS
选择器。自家应用的官方创建接口尚未核实，因此未调用。图标、资质上传、类目选择、
实名、协议、付费和最终提交由人完成。清单必须明确小程序或小游戏；操作者需打开
对应类型的创建表单，工具不会替你猜类型。

`confirmed` 仅代表人已核对并登记应用编号。导出始终标注 `platform_verified: false`；
导入、填表均不代表注册成功。

## 分发

沿用现有 r_lit Jenkins worker 构建、R2 与 HFrog 分发链路，工具名为 `mini_reg`。
下载地址须等正式发布成功后才有效。新增源码不等于已经发布。
SQLite 从随包源码静态链接；HTTP 使用 rustls，不依赖 OpenSSL。
仍需要系统基础库和企业 Chrome。

## 快速使用

```bash
mini_reg init batch.toml
# 修改清单中的公司、任务编号、名称、简介和应用类型。
mini_reg import batch.toml
mini_reg list --json
mini_reg next --json
```

应用类型明确填写 `kind = "mini_program"` 或 `kind = "mini_game"`。
模板包含四个平台各一个任务。批次编号和任务编号是持久标识；同编号更换内容会
整批拒绝，不会导入一半。不同应用应使用新任务编号。

## 绑定企业浏览器

使用既有企业 Chrome 独立进程。由浏览器所有者用独立数据目录与
`--remote-debugging-port=0` 启动；该目录会生成 `DevToolsActivePort`。
调试端口只供本机使用，不向局域网开放。工具不提供启动默认或个人浏览器的命令。

```bash
mini_reg browser bind --user-data-dir /绝对路径/企业浏览器数据 --profile-directory Company
mini_reg browser tabs --json
```

绑定会核对企业账号元信息、单配置清单与当前调试进程指纹。每次操作重新核对。
浏览器重启后需要重新绑定。仅核对账号身份元信息；不读取或导出浏览器的
Cookie、密码和认证令牌库。无法核实企业身份时停止。

## 单个应用的人工接手点

`mini_reg next --open` 按批次、任务编号顺序继续首个未确认任务。
仅在没有页面记录时打开官方入口。已确认任务自动跳过，中断任务保留待检查。
原页面已关闭时，用 `open 任务编号` 明确重开。`next --batch 编号` 可限定一个批次。

```bash
mini_reg open wechat-demo
# 在该页面登录，打开与清单应用类型对应的创建表单。
mini_reg fill wechat-demo
mini_reg fill wechat-demo --apply
# 自行核对字段，选择类目，并完成平台要求的确认。
mini_reg confirm wechat-demo --app-id wx0123456789abcdef
mini_reg export results.json
```

不带 `--apply` 只预览，不修改表单值或任务状态。
可用 `--tab 页面编号` 明确选择 `browser tabs` 列出的页面；否则复用任务记录的页面。
工具不会点击提交或接受协议。缺少可选简介框会报告跳过；名称框必须存在。
跨源框架和封闭影子节点中的输入框需人工填写，不绕过页面边界。

平台表单变化时，持久保存明确的选择器，不另拼临时脚本。
选择器仍必须唯一指向可见、可写的文本框：

```bash
mini_reg selectors --platform wechat --name '#app-name' --summary '#app-summary'
# 不传选择器可恢复该平台的明确标签匹配。
mini_reg selectors --platform wechat
```

## 状态、恢复与输出

默认状态库位于系统应用数据目录中的 `mini_reg` 目录。
所有命令均可通过 `--db /绝对路径/state.sqlite3` 指定位置。
SQLite 事务、WAL 和 FULL 同步保护已提交进度。
任务状态依次为 `planned -> opened -> needs_review -> waiting_human -> confirmed`。
浏览器写入前先提交 `needs_review`；报错或中断后，先检查页面，再明确执行填表。
不会自动重试最终注册请求。

`--json` 只在标准输出写一个结果。日志写标准错误和每次覆盖的边车文件：
默认是状态库同名 `.log`，初始化和导出时是输出文件同名 `.log`。
不记录完整命令参数和浏览器请求内容。命令失败返回 1，并保存错误日志。
清单与导出文件不覆盖已有文件。状态库和导出属于公司内部资料，不放密钥。

迁移状态库时，使用 SQLite 备份接口；或等全部进程退出且 WAL 完成回写后再复制。
存在独立 WAL 时，不要只复制正在使用的主库文件。

## 开发验证

仅在专用 CI worker 编译与测试，禁止在 Jenkins controller、生产或弱主机编译。
本工具通过独立 `[workspace]` 与根目录当前的桌面工具工作区隔离。
在 worker 内执行：

```bash
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
```

测试覆盖重启续办、重复导入、整批回滚、中断状态、编号冲突、密钥字段拒绝、
官方地址边界、企业浏览器核对，以及命令输出和导出。
这些测试不能代替真实渠道注册验收。架构见 [说明](docs/ARCHITECTURE.md)。
