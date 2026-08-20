# MCP 集成

> 文档状态：定稿 | 阶段：系统设计 | 拆分自：原 07-MCP与Skills与子Agent.md

---

## 一、MCP Client

Agent 通过 `McpToolBridge` 将 MCP server 暴露的工具透明适配为内部 `Tool` trait，Agent 无需感知工具来源。

```rust
// crates/agent-core/src/tools/mcp/bridge.rs

pub struct McpToolBridge {
    client: Arc<McpClient>,
    tool_def: mcp_types::Tool,
}

#[async_trait]
impl Tool for McpToolBridge {
    fn name(&self) -> &str { &self.tool_def.name }
    fn description(&self) -> &str { &self.tool_def.description }
    fn input_schema(&self) -> Value { self.tool_def.input_schema.clone() }

    async fn execute(&self, input: ToolInput) -> Result<ToolOutput, ToolError> {
        self.client.call_tool(&self.tool_def.name, input).await
    }
}
```

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
