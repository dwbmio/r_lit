# mailctl

多邮箱 IMAP/SMTP 分发器：一个配置目录，按**邮箱身份**收发信，带白名单 / 发前确认 / 审计。
纯 Rust 单二进制（rustls + webpki-roots），不依赖 OpenSSL 与解释器。

## 能力清单

```bash
mailctl capabilities --json | jq .      # 与本文同源，由二进制直接输出
```

| 命令 | 作用 | 联网 | 副作用 |
|---|---|---|---|
| `accounts` | 列账号（端点/凭证来源/来源文件/签名首行），`--json` | 否 | 无 |
| `folders` | 列邮箱目录（UTF-7 解码中文） | IMAP | 无 |
| `verify` | 登录自测，不发信 | IMAP+SMTP | 无 |
| `scan` | 只读扫邮件头（`--limit --unseen --folder --criteria --match --json`） | IMAP | 无 |
| `read` | 读正文（GBK/GB18030/UTF-8、嵌套 multipart） | IMAP | 无 |
| `draft` | 出草稿（自动签名），不发 | 否 | 写 .eml |
| `reply` | 按 UID 出线程内回复草稿（继承 In-Reply-To/References） | IMAP | 写 .eml |
| `send` | 真发（必须 `--yes`，受白名单/身份校验） | SMTP(+IMAP) | 外发 + 审计 |
| `selftest` | 闭环自测：自发自收回读，不碰第三方 | IMAP+SMTP | 自发一封 |
| `capabilities` | 输出能力清单 JSON | 否 | 无 |

## 四道闸

1. 凭证只在钥匙串/环境变量/不回显输入；配置文件无密码。
2. 白名单按账号隔离，不匹配拒发（exit 1），无命令行绕过；自发自收放行。
3. `send` 必须 `--yes`（exit 3）。
4. `.eml` 的 From 必须等于账号地址。

## 配置：一个邮箱一个文件

`~/.config/hermes-mail/accounts/<账号名>.toml`，文件名即账号名。
`*.toml` 生效，`*.toml.example` 不生效。

```toml
[account]
provider = "qq"
address  = "416646744@qq.com"
auth.keychain_service = "mail-qq-personal"
allow_recipients = ["apple.com", "*.apple.com"]
signature = """
段文彬 / Duan Wenbin
运维负责人 · Head of Operations
Mujin Technology Co., Ltd.（Gamesci · Picboo）
"""
```

## 常用

```bash
security add-generic-password -U -a 416646744@qq.com -s mail-qq-personal -w '<授权码>'
mailctl verify -a qq-personal && mailctl selftest -a qq-personal --wait 90
mailctl scan -a qq-personal --match apple --json
mailctl reply -a qq-personal --uid 3016 --body-file b.txt --out /tmp/r.eml
mailctl send -a qq-personal --eml /tmp/r.eml --yes --append-sent
```

退出码：0 成功 / 1 运行错误 / 2 缺凭证 / 3 未带 `--yes`。

## 边界

- 收信走 IMAP（POP3 无目录、无 UID、无法回读已发送）。
- QQ 会改写 Message-ID（线程靠 References）；部分厂商 SMTP 发信不进「已发送」，需 `--append-sent`。
- 未实现：附件、HTML 正文、IMAP IDLE。
