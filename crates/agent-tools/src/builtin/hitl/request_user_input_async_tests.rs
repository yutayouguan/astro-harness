use super::*;

#[test]
fn parser_accepts_structured_questions() {
    let parsed = parse_async_user_message(
        r#"{"astro_async_user_message":true,"message":"Choose\n- A\n- B","questions":[{"title":"Choose","options":["A","B"]}]}"#,
    )
    .unwrap();

    assert_eq!(parsed.message, "Choose\n- A\n- B");
    assert_eq!(
        parsed.questions,
        Some(vec![AsyncUserInputQuestion {
            title: "Choose".into(),
            options: Some(vec!["A".into(), "B".into()]),
        }])
    );
}

#[test]
fn parser_accepts_legacy_trimmed_async_message_marker() {
    let parsed = parse_async_user_message(
        r#"{"astro_async_user_message":true,"message":"  still working  "}"#,
    )
    .unwrap();

    assert_eq!(
        parsed,
        AsyncUserMessagePayload {
            message: "still working".into(),
            questions: None,
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

#[test]
fn structured_validation_rejects_empty_questions_and_options() {
    assert!(validate_questions(&[]).is_err());
    assert!(validate_questions(&[AsyncUserInputQuestionArgs {
        title: " ".into(),
        options: None,
    }])
    .is_err());
    assert!(validate_questions(&[AsyncUserInputQuestionArgs {
        title: "Choose".into(),
        options: Some(vec![]),
    }])
    .is_err());
}
