//! Assemble the exact native Responses input used by the Agent runtime.

use agent_protocol::ResponseItem;

/// Merge role-bearing prompt context with canonical Responses history.
/// Conversation items are cloned verbatim; only Astro-authored context is
/// inserted at its recorded user boundary.
pub(crate) fn to_response_items_with_context_history(
    prompt_context: &[crate::prompt::context_state::PromptContextEvent],
    session: &[ResponseItem],
) -> Vec<ResponseItem> {
    let session = super::sanitize::sanitized_response_items(session);
    let mut context_events = prompt_context.iter().peekable();
    let context_count = prompt_context
        .iter()
        .map(|event| event.messages.len())
        .sum::<usize>();
    let mut input = Vec::with_capacity(session.len() + context_count);
    let mut user_ordinal = 0usize;
    for item in &session {
        if matches!(item, ResponseItem::Message { role, .. } if role == "user") {
            while context_events
                .peek()
                .is_some_and(|event| event.before_user <= user_ordinal)
            {
                input.extend(
                    context_events
                        .next()
                        .expect("peeked context event")
                        .messages
                        .iter()
                        .filter(|item| matches!(item.role(), Some("developer" | "user")))
                        .cloned(),
                );
            }
            user_ordinal += 1;
        }
        input.push(item.clone());
    }
    for event in context_events {
        input.extend(
            event
                .messages
                .iter()
                .filter(|item| matches!(item.role(), Some("developer" | "user")))
                .cloned(),
        );
    }
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_context_at_the_original_user_boundary_without_rewriting_history() {
        let history = vec![
            ResponseItem::user_text("first"),
            ResponseItem::assistant_text("answer"),
            ResponseItem::user_text("second"),
        ];
        let context = vec![crate::prompt::context_state::PromptContextEvent::new(
            1,
            vec![ResponseItem::developer_text("updated rules")],
        )];

        let input = to_response_items_with_context_history(&context, &history);

        assert_eq!(input.len(), 4);
        assert_eq!(input[0], history[0]);
        assert_eq!(input[1], history[1]);
        assert_eq!(input[2].role(), Some("developer"));
        assert_eq!(input[2].text(), "updated rules");
        assert_eq!(input[3], history[2]);
    }
}
