//! 结构化抽取（`Extractor` / `parse_submit_payload`）测试。

use providers::extractor::{parse_submit_payload, ExtractionError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Person {
    name: Option<String>,
    age: Option<u8>,
    profession: Option<String>,
}

#[test]
fn parse_submit_from_fenced_json() {
    let raw = r#"```json
{"name":"Ada","age":36,"profession":"mathematician"}
```"#;
    let person: Person = parse_submit_payload(raw).unwrap();
    assert_eq!(person.name.as_deref(), Some("Ada"));
    assert_eq!(person.age, Some(36));
}

#[test]
fn parse_submit_from_bare_json_object() {
    let raw = r#"{"name":"Lin","profession":"engineer"}"#;
    let person: Person = parse_submit_payload(raw).unwrap();
    assert_eq!(person.name.as_deref(), Some("Lin"));
    assert!(person.age.is_none());
}

#[test]
fn parse_submit_no_data() {
    let err = parse_submit_payload::<Person>("抱歉，我无法提取。").unwrap_err();
    assert!(matches!(err, ExtractionError::NoData));
}

#[test]
fn parse_submit_deserialization_error() {
    let raw = r#"{"age":"not-a-number"}"#;
    let err = parse_submit_payload::<Person>(raw).unwrap_err();
    assert!(matches!(err, ExtractionError::DeserializationError(_)));
}

#[test]
fn parse_submit_rejects_legacy_tool_markup() {
    let raw = r#"<tool_call>{"name":"submit","arguments":{"name":"John"}}</tool_call>"#;
    let err = parse_submit_payload::<Person>(raw).unwrap_err();
    assert!(matches!(err, ExtractionError::NoData));
}
