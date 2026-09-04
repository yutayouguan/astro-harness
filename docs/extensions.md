# Astro Extension Manifest

> 状态：当前实现基线 | 更新：2026-09-04

Astro 扩展包使用 `extension.toml` 同时声明 MCP Server、Skill、配置 Schema 和已编译工具集合。`agent-extensions` crate 负责发现、校验、合并和生成不可变 `ExtensionSnapshot`；`agent-core` 在每个 turn 的准备边界发布该快照，同一 turn 的所有模型采样与工具调用复用同一版本，磁盘变化从下一 turn 生效。

## 目录

全局扩展：

```text
~/.astro/extensions/<extension-id>/extension.toml
```

可信项目扩展：

```text
<project>/.astro/extensions/<extension-id>/extension.toml
```

项目扩展只有在项目被标记为 `trusted` 后才会加载。同 id 的项目扩展整体覆盖全局扩展。

## Manifest v1

```toml
schema_version = 1
id = "azure-image"
name = "Azure Image"
version = "1.0.0"
description = "Azure GPT Image tools and guidance"
enabled = true

[config]
schema = "config.schema.json"

[config.defaults]
deployment = "gpt-image-2"
size = "1024x1024"

[[skills]]
path = "skills/azure-image"
enabled = true

[[tools]]
toolset = "image_gen"
enabled = true

[mcp_servers.assets]
command = "node"
args = ["server.mjs"]
cwd = "."
env_vars = ["AZURE_API_KEY"]
```

用户或可信项目配置可覆盖扩展默认值：

```toml
[extensions.azure-image]
deployment = "gpt-image-2"
size = "1536x1024"
```

v1 会解析并校验 JSON Schema，并在合并 defaults 与用户/项目配置后验证最终配置。Schema 或配置不合法时，整个扩展包会被跳过并产生诊断；运行时不解释任意业务字段。

## 安全边界

- Manifest 路径必须相对扩展根目录，解析后不能逃逸该目录。
- 项目扩展遵循现有 `ProjectTrust`。
- `agent-core` 只在配置加载器明确返回 `Trusted` 时向扩展解析器传入项目根；扩展解析器还会验证该根是工作目录的祖先。
- JSON Schema 校验不解析 HTTP 或扩展目录之外的外部引用；需要复用的定义应放在同一 Schema 的 `$defs` 中。
- MCP Server id 会加上 `ext-<extension-id>-` 命名空间。
- `tools` 只能启用已经编译进 Astro 的 toolset，不加载动态库或任意原生处理器。
- 新的可执行工具应由 MCP Server 提供；Skill 负责说明、编排和按需激活。
- 单个无效扩展整体跳过并产生诊断，不会留下部分贡献。

## 生效时机

`ExtensionSnapshot` 包含该 turn 使用的：

- 分层配置版本；
- 合并后的 MCP Server 配置；
- Skill 路径和 prompt 索引；
- 配置 JSON Schema 与解析后的默认值/覆盖值；
- toolset 贡献；
- 扩展诊断和稳定内容指纹。

在 turn 运行期间修改 manifest、Skill 或配置不会改变已经发布的贡献集合。下一 turn 会重新发现并发布新快照。

## Reconcile 生命周期

`reconcile_extension_snapshots(previous, next)` 对完全校验后的 immutable snapshot 做内容
指纹比较，按扩展 ID 返回 `added | updated | removed`，并分别标记是否需要刷新 MCP、Skills、
Hooks 和 toolsets。比较失败或新扩展解析失败不会修改当前 active snapshot。

```text
Desktop ReconcileExtensions RPC
  -> Session::reconcile_extensions
  -> discover + validate next snapshot
  -> compare stable fingerprints
  -> store pending_extension_snapshot
  -> return changed ids + affected capabilities
  -> current turn continues with frozen snapshot
  -> next turn publishes pending snapshot once
```

当前 turn 的 `TurnContext` 使用 `OnceLock<Arc<ExtensionSnapshot>>`，同一 turn 内所有 step、
MCP reload、Skill prompt 和 toolset 投影必须共享同一对象。安装、升级或删除造成的磁盘变化
不能在 active turn 中途替换工具集合。

## 与 MCP event stream 的关系

扩展删除或 MCP Server 配置变化会触发 Hub reload。被删除/禁用的 Server 会关闭其连接和
已由 manager 接管的 event streams；未变化的 Server 继续复用。进程级
`McpEventStreamManager` 已实现与 turn task 解耦的所有权、权限 generation 和 Server 删除
取消；具体 MCP Server opener 与 Desktop 订阅入口仍是后续接线项。

## Remote Marketplace 边界

Astro 当前只激活本地用户扩展与可信项目扩展，不把 MCP 公共目录冒充为 Extension
Marketplace，也不调用 Codex 的 ChatGPT 私有 `/ps/plugins/*` 服务。远端安装需要先确定
Astro 自有认证、catalog schema、bundle digest/签名和事务发布协议。设计门槛见
[Remote Extension Marketplace 对齐设计](superpowers/specs/2026-09-04-remote-marketplace-alignment-design.md)。
