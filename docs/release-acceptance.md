# macOS 正式安装包验收记录

日期：2026-09-09。代码检查及隔离配置测试基线：`56ea10ece09a702e93f17eabaddb758c73ccad0a`。

## 结论

**正式分发验收未通过，当前停在发行前置条件。** 配置保留和迁移保护的自动化检查通过，
但没有可验证的正式候选 DMG，也没有完成正式包的干净安装、旧版本覆盖升级或签名自动更新。
此前初始化 QA 包的原生交互验收不能替代这些检查。

本次没有覆盖正式应用、迁移真实 `~/.astro`、导出私钥、触发发布工作流、上传产物，
也没有修改系统 Gatekeeper 设置。

## 安装包与签名检查

| 项目 | 当前证据 | 判定 |
| --- | --- | --- |
| 本地发行候选 | 检查的 `target/release/bundle` / `target/debug/bundle` 中只有 `Astro Onboarding QA.app`，未取得正式 DMG | 待提供候选包 |
| QA 包签名 | `codesign -dvvv` 显示 ad-hoc / linker-signed，TeamIdentifier 未设置、Info.plist 未绑定、资源未封印，未显示 hardened-runtime 标志 | 仅用于本地测试 |
| 严格签名验证 | `codesign --verify --deep --strict` 报 `code has no resources but signature indicates they must be present` | 不满足正式分发 |
| 公证票据 | `xcrun stapler validate` 报无 stapled ticket | 未确认公证通过；不能仅凭缺少票据推断服务端公证记录 |
| 本机签名身份 | `security find-identity -p codesigning -v` 仅列出 Apple Development，没有有效 Developer ID Application | 缺少本地分发签名身份 |
| Gatekeeper | `spctl -a -vv` 虽返回 accepted，但同时标明 `override=security disabled` | 该结果不计作干净系统验收通过 |

QA 包标识为 `ai.astro.onboarding.bundleqa`，正式应用配置为
`com.astroagent.desktop`，不能将前者直接作为正式候选包。

## 发布和更新链路检查

- 实际发布文件是 `.github/workflows/release-tauri.yml`。它接入了 Tauri 更新签名密钥和
  Release token，但没有传入 Apple 分发证书、签名身份或公证认证配置，也没有独立的
  Apple 签名/公证步骤。设计文档中的示例不代表工作流已接入。
- `tauri.conf.json` 的开发占位 `plugins.updater.pubkey` 为空，但运行时代码通过
  `include_str!("../updater.pub")` 显式注入公钥。公钥可解码为 42 字节 minisign key，
  不是“更新公钥缺失”。
- 根据已知存储位置，只核对了更新私钥文件的存在、大小和 0600 权限，以及对应钥匙串
  条目是否存在；没有读取私钥/密码，也没有验证密钥对匹配或实际签名产物。
- 本机未设置本次检查的 Apple/Tauri 签名环境变量。这不等于 CI secrets 或其他钥匙串
  公证 profile 不存在；本次未读取或修改远端 secrets。
- 当前代码和工作流指向 `yutayouguan/astro-agent-releases`。
  公开 GitHub API 的 latest-release 查询以及默认 `latest.json` URL 均返回 HTTP 404。
  另核对过历史使用的 `yutayouguan/astro-agent` latest-release，亦为 404。
  因此本次未取得公开更新候选；404 不足以区分无发布、私有资源或地址配置问题。

## 配置保留与迁移保护

当前工作树存在其他未提交改动；直接测试被
`crates/agent-home/src/storage_cleanup.rs` 的 `String` / `&str` 类型错误阻塞。
未修改该并行任务的代码，改在上述已提交基线的独立源码快照中测试。

| 验证 | 结果 | 证明范围 |
| --- | --- | --- |
| `python3 -m unittest discover -s tools -p test_migrate_home_layout.py -v` | 13 通过 | 历史字节保留、精确资源路径迁移、备份、冲突、符号链接及中断保护 |
| `cargo test -p memory --test home_layout_test` | 4 通过 | 干净初始化、再次启动保留会话/人格/偏好、旧布局拒绝、配置迁移后重启与并发写入 |
| `node tools/verify-settings-restart.mjs` | 4 个独立进程阶段通过 | Desktop 实际写入后，重启的 Desktop、Cron、Workflow 读取相同配置 |

以上使用临时数据根和测试值，不调用真实模型服务。它们证明代码级数据保留契约，
**不代表从已发布旧安装包到新安装包的覆盖升级已通过**。
旧 JSON / 旧目录依照现行契约必须先显式离线迁移；不能用覆盖安装或自动更新来冒充迁移。

## 继续验收需要

1. 明确候选版本，并取得对应的正式 DMG 和用于覆盖升级的旧版本安装包。
2. 准备 Developer ID Application 证书及私钥，通过钥匙串/CI secrets 配置公证认证；
   不在聊天、仓库或日志中传递私钥和密码。
3. 经授权补齐发布工作流的 Apple 签名、公证与失败拦截；确认实际更新发布地址和签名产物。
   Apple 代码签名与 Tauri 更新签名是两个独立检查，不能互相替代。
4. 在默认安全策略启用的干净 macOS 测试环境验证签名、票据与首次打开；
   不修改本机全局安全策略来取得“通过”。
5. 用合成配置、会话、人格文件及其哈希建立升级前基线，再分别验证新安装、
   显式迁移后的覆盖升级、已配置安装的更新与再次启动。保留回滚备份，不覆盖真实用户数据。
