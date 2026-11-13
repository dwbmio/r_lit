# 显式不可变普通归档

旧 `publish` 保持既有版本拼法、latest 和安装脚本行为。本改动仅新增 `publish --archive`；CI 调用者必须显式选择。

普通归档 `is_latest=false`。平台类型不能重复，对象 key 不能重复，已推广版本拒绝修改。已有版本/平台或 R2 对象不同字节直接失败，不覆盖、不降级。R2 用条件创建；相同字节可重入。

生成确定的公开 manifest，登记后回读 HFrog URL/size/SHA，再实际流式下载每件制品和 manifest。APK 与 AAB 使用不同平台，原有 AAB 消费平台可保持不变。

新增错误处理仅输出阶段和异常类型，不回显客户端认证 argv。归档不写稳定 install.sh，不改部署或服务端。SSOT 在本仓；CI 副本按固定 commit/正文 SHA256 同步。

验证：`python3 -B -m unittest discover -s scripts -p test_hfrog_publisher.py`，10项通过。覆盖旧显式发布兼容、HFrog缺失与业务错误、推广/平台冲突、对象 key 冲突、条件创建与竞争、实际下载错误、确定清单及异常脱敏。测试使用假服务，不代表本轮制品已上传。

此分支不合并保护分支，不触发构建，不发布 latest。实际补档由 ci-all-in-one 的独立 feature 负责，回读字节后才验收。
