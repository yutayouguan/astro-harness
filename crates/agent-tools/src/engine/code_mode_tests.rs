use serde_json::json;

use super::render_tool_description;

#[test]
fn renders_function_schema_as_typescript_declaration() {
    let description = render_tool_description(
        "calendar_create_event",
        "创建日程。",
        &json!({
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "日程标题"},
                "duration-minutes": {"type": "integer"},
                "visibility": {"enum": ["private", "public"]},
                "attendees": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title", "duration-minutes"]
        }),
        None,
    );

    assert_eq!(
        description,
        concat!(
            "创建日程。\n\n",
            "exec tool declaration:\n",
            "```ts\n",
            "declare const tools: { calendar_create_event(args: {\n",
            "  attendees?: Array<string>;\n",
            "  \"duration-minutes\": number;\n",
            "  // 日程标题\n",
            "  title: string;\n",
            "  visibility?: \"private\" | \"public\";\n",
            "}): Promise<unknown>; };\n",
            "```"
        )
    );
}

#[test]
fn renders_freeform_tool_as_string_input() {
    let format = types::FreeformToolFormat {
        r#type: "grammar".to_string(),
        syntax: "lark".to_string(),
        definition: "start: /.+/".to_string(),
    };
    let description =
        render_tool_description("apply_patch", "应用补丁。", &json!({}), Some(&format));

    assert!(description.contains("apply_patch(input: string): Promise<unknown>"));
}
