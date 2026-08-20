# mcp

MCP（Model Context Protocol）客户端实现：配置加载、进程级连接池（`McpHub`）、工具发现与调用、OAuth 认证。基于 `rmcp` SDK 支持 stdio 子进程和 Streamable HTTP 两种传输。

## 核心职责

- **配置管理** -- 多层配置加载（全局 / 项目 / Agent 级 `mcp_servers.json`），支持 inline 解码、discovered 工具合并与持久化
- **连接池（McpHub）** -- 按 Agent 维护 MCP Server 连接：并行启动（上限 `MAX_PARALLEL_MCP_STARTUPS=4`）、自动重连退避、生命周期状态管理
- **工具发现** -- 连接成功后自动拉取 Server 工具列表，合并 discovered tool 配置（启用/禁用/超时），暴露 `ToolEntrySpec` 给 ToolRegistry
- **工具调用** -- `call_tool_with_peer()` 执行远程工具调用，支持超时控制和沙盒策略
- **限定名约定** -- `mcp__{server_id}__{tool_name}` 格式，自动 sanitize server_id 中的 `__` 避免歧义
- **Resources / Prompts** -- 显式按需适配 MCP resources 和 prompts 协议扩展
- **OAuth 认证** -- `auth` 模块支持 keyring 凭据存取和 HTTP Bearer 认证

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | 公共 re-exports |
| `config.rs` | 配置加载核心（49KB）：`McpServerConfig` / `McpTransportType`（Stdio / StreamableHttp）/ `McpToolConfig` / `McpToolSettings`、多层加载（`load_mcp_servers_layered`）、inline 解码、discovered 合并与持久化、超时常量 |
| `hub.rs` | 连接池核心（82KB）：`McpHub`（per-Agent 连接池）、`McpExecutionContext`（沙盒策略）、Server 启动/停止/重连、工具列表拉取、`call_tool_with_peer()`、`ToolEntrySpec`、`ServerStatus`、`McpLifecycleState`、`McpServerInstructions`、required servers 校验 |
| `names.rs` | 限定名工具：`qualify_tool_name()` / `parse_qualified_name()` / `is_mcp_tool_name()` / `sanitize_server_id()`，常量 `MCP_PREFIX` / `MCP_TOOLSET` |
| `protocol.rs` | Resources / Prompts 适配：`call_resource_broker()` / `call_prompt_broker()` / `MCP_RESOURCES_TOOL` / `MCP_PROMPTS_TOOL` |
| `auth.rs` | OAuth 认证：keyring 凭据存取、`McpHttpAuth` HTTP Bearer 头注入 |

## 核心类型与 API

- `McpHub` -- per-Agent MCP 连接池，`start_servers()` / `stop_all()` / `call_tool()` / `list_tools()` / `server_status()`
- `McpServerConfig` -- 单个 Server 配置：transport 类型、命令/URL、环境变量、超时、启用状态
- `McpTransportType` -- 传输类型枚举：`Stdio`（子进程）/ `StreamableHttp`（HTTP SSE）
- `McpExecutionContext` -- 连接建立时的沙盒策略快照
- `ToolEntrySpec` -- 暴露给 ToolRegistry 的工具条目：限定名 + JSON Schema + 描述
- `ServerStatus` -- Server 连接状态：Starting / Connected / Failed / Stopped
- `McpLifecycleState` -- 生命周期状态机
- `qualify_tool_name(server_id, tool_name)` -- 生成限定名 `mcp__{sid}__{tool}`
- `parse_qualified_name(name)` -- 解析限定名为 `(server_id, tool_name)`
- `is_mcp_tool_name(name)` -- 判断是否为 MCP 限定工具名

## 设计要点

- **并行启动** -- `MAX_PARALLEL_MCP_STARTUPS=4` 限制同时启动的 Server 数量，避免资源争抢
- **重连退避** -- 连接断开后自动重连，退避上限 `MAX_MCP_RETRY_DELAY_SECS=30`
- **Instructions 上限** -- 单 Server 16KB、所有 Server 总计 64KB，超限截断并保留开头
- **沙盒隔离** -- `McpExecutionContext` 在连接建立时快照沙盒策略，常驻连接不承接单次工具审批
- **Server ID 安全** -- `sanitize_server_id()` 将 `__` 压成 `_`，确保限定名解析无歧义

## Crate 关系

| 方向 | crate |
|------|-------|
| 依赖 | `home`（数据根路径）、`types`（共享类型）、`sandbox`（沙盒策略） |
| 被依赖 | `agent`（核心运行时每轮 reload MCP 工具）、`tools`（MCP 工具分发）、`server`（gRPC 暴露 ListMcpServers） |

## 测试

```bash
# 全部测试
cargo test -p mcp

# 限定名往返测试
cargo test -p mcp roundtrip -- --nocapture
```
