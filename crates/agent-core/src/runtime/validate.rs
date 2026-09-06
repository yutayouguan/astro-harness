use agent_protocol::ResponseItem;

/// 校验消息序列是否满足「相邻不同角色」约束。
///
/// 连续两条 user 或连续两条 assistant 均视为非法，返回 `false`。
pub fn validate_message_order(items: &[ResponseItem]) -> bool {
    let roles = items
        .iter()
        .filter_map(ResponseItem::role)
        .filter(|role| matches!(*role, "user" | "assistant"));
    for (a, b) in roles.clone().zip(roles.skip(1)) {
        if a == b {
            return false;
        }
    }
    true
}
