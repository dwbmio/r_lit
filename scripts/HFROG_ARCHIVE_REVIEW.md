# 显式不可变普通归档

旧 `publish` 保持既有版本拼法、latest 和安装脚本行为。本改动仅新增 `publish --archive`；CI 调用者必须显式选择。

普通归档 `is_latest=false`。平台类型不能重复，对象 key 不能重复，已推广版本拒绝修改。已有版本/平台或 R2 对象不同字节直接失败，不覆盖、不降级。R2 用条件创建；相同字节可重入。

生成确定的公开 manifest，登记后回读 HFrog URL/size/SHA，再实际流式下载每件制品和 manifest。APK 与 AAB 使用不同平台，原有 AAB 消费平台可保持不变。

新增错误处理仅输出阶段和异常类型，不回显客户端认证 argv。归档不写稳定 install.sh，不改部署或服务端。SSOT 在本仓；CI 副本按固定 commit/正文 SHA256 同步。

验证：`python3 -B -m unittest discover -s scripts -p test_hfrog_publisher.py`，14项通过。覆盖旧显式发布兼容、HFrog缺失与业务错误、推广/平台冲突、对象 key 冲突、不同目录同 basename 异内容、保留 manifest 名和输入不被 manifest 覆盖、条件创建与竞争、实际下载错误、确定清单、错误脱敏和 SDK 事件清理。冲突均在远程 GET/POST/上传及本地 manifest 写入前拒绝。单元测试使用假服务；另由 ci-all-in-one CI29 实际完成四件原字节及三个 manifest 存档，全部流式下载 SHA256 验证一致，ERROR/WARNING0。

专用 worker 现有 boto3/botocore 1.34.46 不含 IfNoneMatch 模型参数，纯内存模型校验实证拒绝。兼容修复采用 [Cloudflare 官方自定义请求头示例](https://developers.cloudflare.com/r2/examples/aws/custom-header/) 的 before-call SDK 事件，设置同一 `If-None-Match: *` HTTP 条件，调用后清理事件。没有换客户端认证路径、安装依赖、去掉条件或开放额外权限。存储错误仅输出 operation/class/code/status，不输出异常正文。

新增 `archive-sdk-check` 使用公开假值，禁用实际网络传输，运行同一个 conditional_put 并检查真实 SDK 的最终签名请求。必须包含 `If-None-Match: *` 且出现在 SignedHeaders 中。实际 ubuntu-hs-main 的 boto3/botocore 1.34.46 已通过，故无需安装新 SDK。稳态归档 wrapper 在上传前运行该门禁；失败即退出，不以 import 成功代替能力检查。旧显式 release wrapper 不运行该归档门禁。

此分支不合并保护分支，不触发构建，不发布 latest。实际补档由 ci-all-in-one 的独立 feature 负责，回读字节后才验收。
