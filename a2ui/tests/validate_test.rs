use a2ui::{validate_operations, ASTRO_CATALOG_ID};

#[test]
fn rejects_unknown_component() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "root", "component": "NotARealWidget", "text": "x" }
                ]
            }
        }
    ]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("NotARealWidget"));
}

#[test]
fn accepts_text_card_button() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["t", "b"] },
                    { "id": "t", "component": "Text", "text": "Hello", "variant": "h2" },
                    {
                        "id": "b",
                        "component": "Button",
                        "child": "bt",
                        "variant": "primary",
                        "action": { "event": { "name": "ok" } }
                    },
                    { "id": "bt", "component": "Text", "text": "OK" }
                ]
            }
        }
    ]);
    validate_operations(ops.as_array().unwrap()).unwrap();
}
