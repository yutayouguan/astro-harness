use super::*;

#[test]
fn parser_accepts_trimmed_async_message_marker() {
    let parsed = parse_async_user_message(
        r#"{"astro_async_user_message":true,"message":"  still working  "}"#,
    )
    .unwrap();

    assert_eq!(
        parsed,
        AsyncUserMessagePayload {
            message: "still working".into(),
        }
    );
}

#[test]
fn parser_rejects_unmarked_or_empty_results() {
    assert_eq!(parse_async_user_message(r#"{"message":"hello"}"#), None);
    assert_eq!(
        parse_async_user_message(r#"{"astro_async_user_message":true,"message":"  "}"#),
        None
    );
}
