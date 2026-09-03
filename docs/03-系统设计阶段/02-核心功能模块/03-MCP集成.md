# MCP 集成

> **Harness 定位（2026-09-03）**：MCP 是 Harness 的外部工具 Gateway，而不是 Model 本身能力。模型侧使用 Responses 原生 `namespace=mcp__{server}` + `name={tool}`，Hub 内部才使用展平执行键。当 `tool_search` 可用时，MCP 工具默认 Deferred；搜索激活只改变模型可见性，不跳过 MCP approval、HITL、`StepContext` 或审计。见 [Agent Harness 总体架构](../01-架构设计/11-Agent-Harness总体架构.md)。

> 文档状态：定稿 | 阶段：系统设计 | 拆分自：原 07-MCP与Skills与子Agent.md

---

## 一、MCP Client

`McpHub` 维持每 Agent 连接池并在 `tools/list` 后生成 `ToolEntrySpec`。
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

---

## 二、MCP 工具风险等级配置

McpToolBridge 默认风险等级为 `Medium`，但支持在 `mcp-servers.toml` 中按 Server 和工具名覆盖：

```toml
[servers.github]
command = "mcp-server-github"
default_risk_level = "low"      # 该 Server 所有工具默认 Low

[servers.github.risk_overrides]
"create_issue" = "medium"       # 特定工具覆盖为 Medium
"delete_repo" = "high"          # 危险操作覆盖为 High
```

**默认风险等级**：未显式配置时，所有 MCP 工具统一默认为 `Medium`，不进行基于名称的自动推断。可通过 `default_risk_level`（Server 级）和 `risk_overrides`（工具级）在配置中显式覆盖。

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

`agent-mcp-server` crate 将 Agent 的 Skill/Tool 能力暴露为标准 MCP 接口，供其他 Agent 或工具调用。

---

## 五、MCP Server 配置

```toml
# mcp-servers/servers.toml

[[servers]]
name      = "filesystem"
transport = "stdio"
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-filesystem", "/workspace"]

[[servers]]
name      = "postgres"
transport = "streamable-http"
url       = "http://localhost:5432/mcp"

[[servers]]
name      = "browser"
transport = "stdio"
command   = "npx"
args      = ["-y", "@modelcontextprotocol/server-puppeteer"]
```

> **传输判定**：存在 `command` 且不存在 `url` 时使用 STDIO；存在 `url` 且不存在 `command` 时使用 Streamable HTTP；同时存在或同时缺失均为配置错误。历史 `transport = "sse"` 返回明确迁移错误，用户必须提供 Server 的 `/mcp` endpoint。

---

## 相关文档

- [04-Skills系统.md](04-Skills系统.md) — Skills 系统设计
- [05-子Agent派生.md](05-子Agent派生.md) — 子 Agent 派生设计
- `04-详细设计阶段/04-工具与扩展生态/03-MCP协议详细设计.md` — MCP 协议详细设计
