# MCP 集成

> **Harness 定位（2026-09-04）**：MCP 是 Harness 的外部工具 Gateway，而不是 Model 本身能力。模型侧使用 Responses 原生 `namespace=mcp__{server}` + `name={tool}`，Hub 内部才使用展平执行键。当 `tool_search` 可用时，MCP 工具默认 Deferred；搜索激活只改变模型可见性，不跳过 MCP approval、HITL、`StepContext` 或审计。见 [Agent Harness 总体架构](../01-架构设计/11-Agent-Harness总体架构.md)。

> 文档状态：定稿 | 阶段：系统设计 | 拆分自：原 07-MCP与Skills与子Agent.md

---

## 一、MCP Client

`McpHub` 维持每 Agent 连接池并在 `tools/list` 后生成 `ToolEntrySpec`；进程级
`McpEventStreamManager` 提供独立于 turn task 的长流所有权基础。
`Session::attach_mcp_tools()` 将条目注册到 `ToolRegistry`：

```text
MCP tools/list
  -> ToolEntrySpec { namespace, native_name, qualified_name, schema, approval }
  -> ToolEntry { namespace, internal name = qualified_name }
  -> Responses namespace schema
  -> StepContext / ToolRouter (namespace, native_name)
  -> qualified_name
  -> McpHub::resolve_tool_peer
  -> peer.call_tool(native_name, arguments)
```

因此 Agent 执行循环无需根据工具名前缀猜测 Server，但调用仍会通过统一的
approval、sandbox、hook、output budget 和持久化链路。

event stream manager 使用 `(thread_id, subscription_id)` 作为稳定所有权键，每次启动分配单调
`stream_attempt_id`。只有收到首条 `notifications/events/active` 才算激活；通知走容量 64 的
有界队列。普通 turn/task 卸载不结束长流，权限 generation 变化、Server 删除/禁用或 Hub
shutdown 会取消对应 worker。当前尚未接入具体 MCP Server opener 和 Desktop 订阅 RPC。

---

## 二、MCP 工具开关与审批

MCP 使用 Server 默认审批模式与单工具覆盖；工具可见性和审批相互独立：

```toml
[mcp_servers.github]
command = "mcp-server-github"
default_tools_approval_mode = "writes"
enabled_tools = ["list_issues", "create_issue"]
disabled_tools = ["delete_repo"]

[mcp_servers.github.tools.create_issue]
enabled = true
approval_mode = "prompt"
output_token_limit = 1200
```

规则顺序是 `enabled_tools` allow list → `disabled_tools` deny list → 单工具 `enabled`。
审批先读取单工具 `approval_mode`，缺失时使用 `default_tools_approval_mode`。annotations
只能辅助审批路由，不能提升基础权限；`output_token_limit` 在原始输出、Hook 变换和最终回灌
后都保持同一个预算上限。

---

## 三、MCP Transport

MCP transport 支持两种方式：

```text
mcp/
└── transport.rs    # 统一传输层（基于 rmcp crate，支持 STDIO / Streamable HTTP）
```

> 传输层基于 `rmcp` crate 实现统一抽象，仅启用 `stdio` 与 `transport-streamable-http-client-reqwest`。旧 `/sse` + `/message` 双端点不受支持，也不会被静默映射为 Streamable HTTP。

> MCP client 按配置建立连接并复用健康 Peer；连接与首次 `tools/list` 受启动 deadline 约束。配置、权限或凭据引用变化时重连，禁用或删除时主动关闭。

---

## 四、MCP Server 暴露

Astro 当前只实现 MCP Client，不提供独立 `agent-mcp-server` crate，也不把 Agent 的内部
Skill/Tool 自动暴露成远端 MCP Server。需要对外暴露时必须另行设计认证、权限与生命周期，
不能复用 client 配置暗中开放监听端口。

---

## 五、MCP Server 配置

```toml
[mcp_servers.filesystem]
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]

[mcp_servers.postgres]
url       = "http://localhost:5432/mcp"

[mcp_servers.browser]
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-puppeteer"]
```

> **传输判定**：存在 `command` 且不存在 `url` 时使用 STDIO；存在 `url` 且不存在 `command` 时使用 Streamable HTTP；同时存在或同时缺失均为配置错误。历史 `transport = "sse"` 返回明确迁移错误，用户必须提供 Server 的 `/mcp` endpoint。

---

## 相关文档

- [04-Skills系统.md](04-Skills系统.md) — Skills 系统设计
- [05-子Agent派生.md](05-子Agent派生.md) — 子 Agent 派生设计
- `04-详细设计阶段/04-工具与扩展生态/03-MCP协议详细设计.md` — MCP 协议详细设计
