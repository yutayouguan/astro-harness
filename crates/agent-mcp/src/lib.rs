//! MCP（Model Context Protocol）客户端：配置、Hub 与工具命名。
//!
//! - [`config`]：服务器配置加载 / 持久化与发现工具合并
//! - [`hub`]：进程连接、工具列表与调用
//! - [`names`]：`mcp__{server}__{tool}` 限定名约定

pub mod config;
pub mod hub;
pub mod names;

pub use config::{
    load_for_active_agent, load_mcp_servers, merge_discovered, persist_discovered,
    save_mcp_servers, DiscoveredTool, McpServerConfig, McpTransportType,
};
pub use hub::{
    call_tool_with_peer, filter_enabled_tool_names, toolset_name, McpExecutionContext, McpHub,
    ServerStatus, ToolEntrySpec,
};
pub use names::{
    is_mcp_tool_name, parse_qualified_name, qualify_tool_name, sanitize_server_id, MCP_PREFIX,
    MCP_TOOLSET,
};
