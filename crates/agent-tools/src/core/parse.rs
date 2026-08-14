//! 工具调用解析：re-export from `types::tool_call`。
//!
//! 具体实现已下沉到 `storage/common`，本模块保留 re-export 以保持下游兼容。

pub use types::tool_call::{
    extract_tool_calls, resolve_tool_calls, ParsedToolCall, ToolCallAccumulator, ToolCallDelta,
};
