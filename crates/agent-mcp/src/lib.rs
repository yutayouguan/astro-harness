//! MCP（Model Context Protocol）客户端：配置、Hub 与工具命名。
//!
//! - [`config`]：服务器配置加载 / 持久化与发现工具合并
//! - [`hub`]：进程连接、工具列表与调用
//! - `protocol`：resources / prompts 的显式按需适配
//! - [`names`]：`mcp__{server}__{tool}` 限定名约定

pub mod auth;
pub mod config;
pub mod hub;
pub mod names;
mod protocol;

pub use config::{
    decode_inline_mcp_servers, load_mcp_servers, load_mcp_servers_layered, load_mcp_servers_scoped,
    mcp_config_path_for_project, mcp_config_path_global, merge_discovered, persist_discovered,
    persist_discovered_layered, save_mcp_servers, save_mcp_servers_scoped, DiscoveredTool,
    McpHttpAuth, McpServerConfig, McpToolConfig, McpToolSettings, McpTransportType,
    DEFAULT_STARTUP_TIMEOUT_SECS, DEFAULT_TOOL_TIMEOUT_SECS, STARTUP_TIMEOUT_SECS_RANGE,
    TOOL_TIMEOUT_SECS_RANGE,
};
pub use hub::{
    call_tool_with_peer, filter_enabled_tool_names, toolset_name, McpBrokerCapabilities,
    McpExecutionContext, McpHub, McpLifecycleState, McpServerInstructions, McpStartupFailure,
    RequiredMcpServersError, ServerStatus, ToolEntrySpec, MAX_MCP_SERVER_INSTRUCTIONS_CHARS,
    MAX_PARALLEL_MCP_STARTUPS, MAX_TOTAL_MCP_INSTRUCTIONS_CHARS,
};
pub use names::{
    is_mcp_tool_name, parse_qualified_name, qualify_tool_name, sanitize_server_id, MCP_PREFIX,
    MCP_TOOLSET,
};
pub use protocol::{
    call_prompt_broker, call_resource_broker, MCP_PROMPTS_TOOL, MCP_RESOURCES_TOOL,
};
