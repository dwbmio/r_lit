# mailctl

多邮箱 IMAP/SMTP 分发器：**一个配置目录，按邮箱身份收发信**，带白名单 / 发前确认 / 审计。
纯 Rust 单二进制（rustls + webpki-roots，无 OpenSSL、无解释器依赖）。

## 能力清单（给人和 AI 共用）

机器可读版本由二进制直接输出，契约与本文同源（`capabilities.json` 内嵌进二进制）：

```bash
mailctl capabilities --json | jq .
```

| 命令 | 作用 | 联网 | 副作用 |
|---|---|---|---|
| `accounts` | 列出账号（端点 / 凭证来源 / 来源文件 / 签名首行），`--json` 机器可读 | 否 | 无 |
| `folders` | 列出邮箱目录（IMAP modified-UTF-7 解码成中文） | IMAP | 无 |
| `verify` | 登录自测 IMAP + SMTP，**不发信** | IMAP+SMTP | 无 |
| `scan` | 只读扫邮件头：`--limit` `--unseen` `--folder` `--criteria` `--match` `--json` | IMAP | 无（EXAMINE + BODY.PEEK） |
| `read` | 读某封正文（GBK/GB18030/UTF-8 + 嵌套 multipart），`--raw` 打印原始报文 | IMAP | 无 |
| `draft` | 新建草稿（自动加本账号签名），不发送 | 否 | 写 `.eml` |
| `reply` | 按 UID 生成线程内回复（自动继承 In-Reply-To / References），不发送 | IMAP | 写 `.eml` |
| `send` | 真发 `.eml`：必须 `--yes`；受白名单 + 身份校验；`--append-sent` 存已发送 | SMTP(+IMAP) | **外发 + 审计行** |
| `selftest` | 闭环自测：给自己发一封并回读收件箱，不外发第三方 | IMAP+SMTP | 自发一封 + 审计 |
| `capabilities` | 输出本能力清单（JSON） | 否 | 无 |

## 四道闸（不可绕过）

1. **凭证**：钥匙串 `keychain_service` > 环境变量 > 不回显交互输入；配置文件里永远没有密码。
2. **白名单**：`allow_recipients` 按账号隔离，不匹配直接拒发（exit 1），**无命令行开关可绕**；自发自收自动放行。
3. **发前确认**：`confirm_before_send = true`（默认）时 `send` 必须带 `--yes`，否则 exit 3。
4. **身份一致**：`.eml` 的 `From` 必须等于账号地址，禁止用 A 身份发 B 的信。

另有：明文 SMTP/IMAP 只允许 `127.0.0.1`（防授权码裸奔）；`scan` / `read` 全程只读，不标已读、不删信。

## 配置：一个邮箱一个文件

```
~/.config/hermes-mail/accounts/
├── qq-personal.toml          # 生效，账号名 = 文件名
└── work-exmail.toml.example  # 模板，不生效
```

```toml
[account]
provider = "qq"                      # qq / exmail / aliyun-enterprise / aliyun / gmail / outlook / custom
address  = "416646744@qq.com"
display_name = "段文彬 | Mujin Technology"
auth.keychain_service = "mail-qq-personal"
sent_folder = "Sent Messages"
allow_recipients = ["apple.com", "*.apple.com"]
signature = """
段文彬 / Duan Wenbin
运维负责人 · Head of Operations
Mujin Technology Co., Ltd.（Gamesci · Picboo）
"""
```

签名也可用 `signature_file = "sig.txt"`（相对该账号文件所在目录）。签名按账号隔离，`--no-signature` 临时关闭。

## 常用流程

```bash
# 首次接入
security add-generic-password -U -a 416646744@qq.com -s mail-qq-personal -w '<授权码>'
mailctl verify   -a qq-personal
mailctl selftest -a qq-personal --wait 90      # 自发自收，闭环验证

# 回复某封
mailctl scan  -a qq-personal --match 'apple' --json
mailctl read  -a qq-personal --uid 3016
mailctl reply -a qq-personal --uid 3016 --body-file body.txt --out /tmp/r.eml
mailctl send  -a qq-personal --eml /tmp/r.eml --yes --append-sent
mailctl scan  -a qq-personal --folder 'Sent Messages' --limit 5
```

## 退出码

| 码 | 含义 |
|---|---|
| 0 | 成功 |
| 1 | 运行错误（连接 / 认证 / 白名单拦截 / 身份不符） |
| 2 | 缺少凭证 |
| 3 | 未带 `--yes`，拒绝发送 |

## 构建与发布

```bash
cargo test            # 25 项：单元 + 端到端（内置假 IMAP/SMTP，不联网）
cargo build --release
```

发布走 Jenkins `r_lit` 流水线（`ci-all-in-one/task/ci/pipeline/r_lit/`），交叉编译矩阵：
`x86_64-unknown-linux-gnu`、`aarch64-unknown-linux-gnu`、`x86_64-unknown-linux-musl`、
`aarch64-apple-darwin`、`x86_64-pc-windows-gnu`。

## 已知边界

- 收信走 IMAP（不用 POP3）：POP3 无目录、无 UID 精确定位、无法回读「已发送」。
- 厂商差异：QQ 投递会**改写 Message-ID**（线程靠 References 维持）；部分厂商 SMTP 发信不自动进「已发送」，需 `--append-sent`。
- 未实现：附件发送、HTML 正文、IMAP IDLE 实时收信（轮询用 `scan --unseen`）。
