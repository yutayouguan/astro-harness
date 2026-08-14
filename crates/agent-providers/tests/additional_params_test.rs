//! `merge_additional_params` 请求体浅合并行为测试。

use providers::merge_additional_params;
use serde_json::json;

#[test]
fn merges_object_fields() {
    let mut body = json!({"model": "gpt", "temperature": 0.7});
    merge_additional_params(&mut body, &json!({"temperature": 0.2, "top_p": 0.9}));
    assert_eq!(body["temperature"], 0.2);
    assert_eq!(body["top_p"], 0.9);
    assert_eq!(body["model"], "gpt");
}

#[test]
fn ignores_non_object_params() {
    let mut body = json!({"model": "gpt"});
    merge_additional_params(&mut body, &json!(["x"]));
    assert_eq!(body, json!({"model": "gpt"}));
}
