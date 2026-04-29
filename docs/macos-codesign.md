# macOS 代码签名 + 公证（Developer ID 分发）

本仓库的 GUI 项目（`maquette`、`group_vibe_workbench`）通过 GitHub Actions
自动产出 **签名 + 公证 + stapled** 的 `.dmg`，可直接发到 GitHub Releases /
HFrog 给最终用户双击安装，无需 Gatekeeper 绕过。

> 流水线: `.github/workflows/release.yml` → `.github/scripts/macos-bundle.sh`
> Bundle 资源: `<project>/macos/Info.plist` + `<project>/macos/<bin>.entitlements`

---

## 一次性准备（约 1 小时）

### 1. 注册 Apple Developer Program — $99/年

- https://developer.apple.com/programs/enroll/
- 个人或公司均可。**不要**选 Enterprise Program（299/年）——见 NOTE 1。

### 2. 在 Mac 上申请 Developer ID Application 证书

1. 打开 **Keychain Access**（钥匙串访问） → 菜单 *Certificate Assistant* →
   *Request a Certificate From a Certificate Authority*。
2. 邮箱填 Apple ID，名字随意，**Saved to disk** 勾上 → 生成 `.certSigningRequest` 文件。
3. 登录 https://developer.apple.com/account/resources/certificates/list ，
   *+* → 选 **Developer ID Application** → 上传刚才的 CSR → 下载
   `developerID_application.cer`。
4. 双击 `.cer` 安装到 *login* 钥匙串。

### 3. 把证书 + 私钥导出为 `.p12`

1. 钥匙串里展开证书，应该能看到下面的 *private key*。
2. 同时选中 *证书* 和 *私钥* → 右键 → *Export 2 items…* → 格式选
   **Personal Information Exchange (.p12)** → 设置一个强密码（**记下来**）。
3. 保存为 `developer-id.p12`。

### 4. 生成 Notarytool 用的 App-Specific Password

- 登录 https://appleid.apple.com → **Sign-In and Security** → **App-Specific
  Passwords** → 生成一个，标签写 `notarytool-r_lit` → 复制 16 位密码。

### 5. 找出你的 Team ID

- https://developer.apple.com/account → 右上角 Membership Details → **Team ID**
  （10 位字母数字）。

### 6. 找出证书的 Common Name

```bash
security find-identity -v -p codesigning
# 输出形如:
#   1) ABC123… "Developer ID Application: Your Name (TEAMID)"
```
完整双引号里的字符串即 `MACOS_CERTIFICATE_NAME`。

---

## 配置 GitHub Secrets

仓库 **Settings → Secrets and variables → Actions → New repository secret**，
逐个加入：

| Secret 名 | 内容 | 示例 |
|---|---|---|
| `MACOS_CERTIFICATE` | `.p12` 文件的 base64：<br>`base64 -i developer-id.p12 \| pbcopy` | `MIIM…长串` |
| `MACOS_CERTIFICATE_PWD` | 上一步导出 `.p12` 时设置的密码 | `your-p12-password` |
| `MACOS_CERTIFICATE_NAME` | 证书 Common Name（含双引号里的全部内容，**不要**带双引号本身） | `Developer ID Application: Your Name (ABCDE12345)` |
| `MACOS_TEAM_ID` | 10 位 Team ID | `ABCDE12345` |
| `MACOS_NOTARY_APPLE_ID` | 你 Apple Developer 账号的邮箱 | `you@example.com` |
| `MACOS_NOTARY_PWD` | 上面生成的 App-Specific Password | `abcd-efgh-ijkl-mnop` |
| `MACOS_KEYCHAIN_PASSWORD` *(可选)* | 临时 keychain 解锁密码（任意字符串，**只在 CI 临时 runner 上用**） | `r_lit-ci-temp` |

> 全部 Secrets 都准备好之后，下次发布版本（任意改 `Cargo.toml` 的 version 推到
> main）就会触发完整签名 + 公证流水线。如果某个 Secret 没设置，CI 会输出
> warning 并 **跳过 GUI 项目的 .dmg 打包**（其它平台 / CLI 项目正常发布）。

---

## 本地手动签名一次（debug / 排错用）

```bash
# 在仓库根目录
export MACOS_CERTIFICATE_NAME="Developer ID Application: Your Name (ABCDE12345)"
export MACOS_TEAM_ID="ABCDE12345"
export MACOS_NOTARY_APPLE_ID="you@example.com"
export MACOS_NOTARY_PWD="abcd-efgh-ijkl-mnop"

# 先本地构建 release 二进制
( cd maquette && cargo build --release --target aarch64-apple-darwin )

# 然后跑打包脚本
APP_NAME="Maquette" \
BIN_NAME="maquette" \
PROJECT_DIR="maquette" \
TARGET_TRIPLE="aarch64-apple-darwin" \
VERSION="0.1.0" \
OUT_DIR="$(pwd)/release" \
  bash .github/scripts/macos-bundle.sh

# 干跑（只签名不公证，省去 Apple 服务器往返）
SKIP_NOTARIZE=1 \
APP_NAME="Maquette" BIN_NAME="maquette" PROJECT_DIR="maquette" \
TARGET_TRIPLE="aarch64-apple-darwin" VERSION="0.1.0" \
OUT_DIR="$(pwd)/release" \
  bash .github/scripts/macos-bundle.sh
```

成功后 `release/maquette-aarch64-apple-darwin.dmg` 就是可分发的最终产物，双击挂载、拖到 Applications 即可启动。

---

## 验证最终产物

```bash
DMG=release/maquette-aarch64-apple-darwin.dmg

# 1. 签名是否完整
codesign --verify --deep --strict --verbose=2 "$DMG"

# 2. 是否被苹果公证（stapled）
xcrun stapler validate "$DMG"

# 3. Gatekeeper 是否放行
spctl -a -t open --context context:primary-signature -vvv "$DMG"
# 期望: source=Notarized Developer ID
```

---

## 常见踩坑

| 症状 | 原因 / 解决 |
|---|---|
| `errSecInternalComponent` 在 codesign 阶段 | 钥匙串没有 unlock，或 set-key-partition-list 缺失。脚本已处理；本地试时手动 `security unlock-keychain login.keychain` |
| Notarytool 卡 *In Progress* 超过 30 分钟 | Apple 端拥堵，10 分钟以上正常；脚本用 `--wait`，CI 6h 超时内可承受 |
| Notarytool 返回 `Invalid` | `xcrun notarytool log <submission-id> --apple-id … --team-id … --password … notarylog.json` 看具体哪条 entitlement / hardened runtime 违规 |
| `bevy_egui` 启动时 GPU 黑屏 | 沙盒 / hardened runtime entitlement 太严，确认 `com.apple.security.cs.allow-unsigned-executable-memory=true` |
| `gpui` 找不到本地节点 | `Info.plist` 必须含 `NSLocalNetworkUsageDescription` + `NSBonjourServices`，本仓库已配 |
| 用户拿到 dmg 双击说"已损坏" | 没 staple 或 staple 失败；用 `xcrun stapler validate` 复核；最坏情况 `xattr -dr com.apple.quarantine /Applications/Maquette.app` 临时绕过 |

---

## NOTE 1 — 为什么不用 Enterprise Program

`Apple Developer Enterprise Program (ADEP, $299/年)` 限制：

- **必须有 100+ 员工 + D-U-N-S 编号**；
- 仅限对自家公司员工分发，**严禁公开/对外分发**；
- 苹果近年（2019+）对滥用证书签 App Store 外公开分发的违规行为大量封号。

我们走的是 **普通 Apple Developer Program ($99/年)** 颁发的
**Developer ID Application** 证书，这是 Apple 官方支持的「在 App Store 之外
对全世界用户分发」的唯一合规渠道，也是 Zed、Postman、Discord、Steam、
Bevy 演示等成千上万 Mac 应用的标准做法。

---

## NOTE 2 — 公证不等于审核

- 「公证」(notarization) 是 **自动化恶意软件扫描 + 签名校验**，
  通常 1–10 分钟出结果；
- 不是 App Store 那种人工审核，**不会**因 UX、定价、协议被拒；
- 只要 hardened runtime + 正确签名，几乎一次过。
