# Remote Extension Marketplace 对齐设计

> 状态：设计审查通过，等待产品服务契约
> Codex 对照范围：`#41949`、`#41953`、`#42114`、`#42149`、`#42150`

## 结论

Astro 不直接调用 Codex 的 `/ps/plugins/*` 私有接口。该接口依赖 ChatGPT 身份、工作区、
安装策略和服务端 bundle 签发，Astro 当前没有等价的认证与服务契约。伪造同名 API 会让
“可安装”“已安装”“管理员禁用”等状态失去可信来源。

现有本地 Extension 发现和 `ReconcileExtensions` 作为安装后的唯一激活路径继续保留。
远端 Marketplace 必须先落地下面的边界，再接 Desktop。

## 目标架构

```text
MarketplaceClient
  -> immutable catalog snapshot (etag/version/source)
  -> ExtensionMarketplaceManager (single mutation gate)
  -> download into fresh staging directory
  -> verify digest, archive bounds, paths and extension.toml
  -> atomic publish under ~/.astro/extensions/<id>
  -> ReconcileExtensions
  -> current turn keeps frozen snapshot; next turn activates it
```

### 服务契约

- `list_marketplaces`：返回 marketplace id、显示名、来源、缓存状态和不可变版本。
- `list_extensions` / `get_extension`：返回远端 ID、稳定本地 ID、版本、能力摘要、安装与可用状态。
- `install_extension` / `upgrade_extension` / `remove_extension`：所有 mutation 由同一个 manager
  串行化，并返回受影响的 MCP、Skills、Hooks、toolsets。
- 所有请求必须走统一出站 HTTP policy；认证头由 credential provider 注入，不写入日志、
  rollout、manifest 或 catalog cache。

### 供应链与回滚

- Catalog 必须提供 bundle digest；未固定 digest 的条目不可安装。
- 下载和解包只允许进入新 staging 目录；限制总字节、文件数、单文件大小和路径深度。
- 拒绝绝对路径、`..`、设备文件、逃逸 symlink/hardlink 和 manifest 外的原生可执行代码。
- 完整解析并验证 `extension.toml` 后才能原子替换已安装版本。
- 安装、升级、删除失败时保留当前已激活目录和 snapshot；成功后才调用 reconcile。
- 删除先移动到隔离目录，reconcile 成功后再回收，避免部分删除。

### 来源策略

- 远端、Git 和本地 marketplace 使用不同 source kind，不按 URL 形状猜测。
- 企业允许列表匹配规范化后的完整 Git URL/固定 ref、远端 catalog ID 或 canonical 本地路径。
- curated 名称是保留名称，不能被用户配置的本地或 Git 源冒充。
- 配置合并后重新校验来源；升级不能绕过初次安装时的来源策略。

## 不采用的方案

- 不把现有 MCP 公共目录改名为 Plugin Marketplace；它没有 extension bundle、版本和事务语义。
- 不让 Desktop 直接下载并覆盖 `~/.astro/extensions`；UI 不是安全边界。
- 不在没有后端身份与签名协议时复用 Codex 私有远端 endpoint。
- 不在失败时删除或降级已激活版本。

## 验收矩阵

- fresh/stale/离线 catalog cache 与 ETag 刷新。
- 安装、升级、删除的成功、下载失败、校验失败、发布失败和 reconcile 失败。
- 同一 extension 并发 mutation 串行化；不同 extension 不互相破坏。
- curated/source policy、workspace 可见性、管理员禁用和认证失败。
- bundle traversal、symlink、超限、digest 不匹配和敏感头日志脱敏。
- 当前 turn 保持旧 snapshot，下一 turn 才看到新版本；任务卸载不损坏已安装目录。

## 实施门槛

进入编码阶段前必须确定 Astro 自有 Marketplace 服务的 base URL、认证方式、catalog JSON
schema、bundle 签名或 digest 信任根，以及 Desktop 产品入口。缺少任一项时只保留本地
Extension 与现有 MCP catalog，不声明远端 Marketplace 已启用。
