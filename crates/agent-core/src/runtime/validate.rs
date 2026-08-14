use types::message::Message;

/// 校验消息序列是否满足「相邻不同角色」约束。
///
/// 连续两条 user 或连续两条 assistant 均视为非法，返回 `false`。
pub fn validate_message_order(messages: &[Message]) -> bool {
    use types::message::Role;
    for window in messages.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        if a.role == Role::User && b.role == Role::User {
            return false;
        }
        if a.role == Role::Assistant && b.role == Role::Assistant {
            return false;
        }
    }
    true
}
