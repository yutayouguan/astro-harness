//! Skills 域统一使用的 Agent id 兼容层。

/// 返回协议/持久化层的规范 Agent id；旧 `workspace` 别名归一为 `default`。
pub(crate) fn normalize(agent_id: Option<&str>) -> String {
    home::normalize_agent_id(agent_id.unwrap_or(home::DEFAULT_AGENT_ID))
}

/// 保留 `None` 的可选版本，用于区分全局配置与 Agent 级配置。
pub(crate) fn normalize_optional(agent_id: Option<&str>) -> Option<String> {
    agent_id
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(home::normalize_agent_id)
}
