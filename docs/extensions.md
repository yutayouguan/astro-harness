# Astro Extension Manifest

Astro 扩展包使用 `extension.toml` 同时声明 MCP Server、Skill、配置 Schema 和已编译工具集合。运行时在每个 turn 的准备边界发现扩展，并生成不可变 `ExtensionSnapshot`；同一 turn 的所有模型采样与工具调用复用同一版本，磁盘变化从下一 turn 生效。

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

v1 会解析并冻结 JSON Schema，供设置 UI 或扩展消费者生成表单和执行校验；核心运行时暂不自行解释任意业务字段。

## 安全边界

- Manifest 路径必须相对扩展根目录，解析后不能逃逸该目录。
- 项目扩展遵循现有 `ProjectTrust`。
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
