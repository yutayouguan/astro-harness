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

#[test]
fn catalog_id_is_v2() {
    assert_eq!(ASTRO_CATALOG_ID, "astro://a2ui/catalog/v2");
}

#[test]
fn rejects_v1_catalog_id() {
    let ops = serde_json::json!([{
        "version": "v0.9",
        "createSurface": {
            "surfaceId": "s1",
            "catalogId": "astro://a2ui/catalog/v1"
        }
    }]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("catalog"));
}

#[test]
fn accepts_extension_components() {
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
                    {
                        "id": "col",
                        "component": "Column",
                        "children": ["badge", "metric", "callout", "avatar", "chip", "sp"]
                    },
                    { "id": "badge", "component": "Badge", "text": "Live", "variant": "success" },
                    { "id": "metric", "component": "Metric", "label": "CPU", "value": "42%", "hint": "ok" },
                    { "id": "callout", "component": "Callout", "text": "注意", "variant": "warn" },
                    { "id": "avatar", "component": "Avatar", "text": "SH" },
                    { "id": "chip", "component": "Chip", "text": "tag" },
                    { "id": "sp", "component": "Spacer", "size": "md" }
                ]
            }
        }
    ]);
    validate_operations(ops.as_array().unwrap()).unwrap();
}

#[test]
fn rejects_deferred_modal() {
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
                    { "id": "m", "component": "Modal", "child": "x" }
                ]
            }
        }
    ]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("Modal"));
}

#[test]
fn delete_surface_requires_surface_id() {
    let ops = serde_json::json!([{
        "version": "v0.9",
        "deleteSurface": {}
    }]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("surfaceId"));
}

#[test]
fn delete_surface_rejects_empty_id() {
    let ops = serde_json::json!([{
        "version": "v0.9",
        "deleteSurface": { "surfaceId": "" }
    }]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("surfaceId"));
}

#[test]
fn delete_surface_accepts_valid_id() {
    let ops = serde_json::json!([{
        "version": "v0.9",
        "deleteSurface": { "surfaceId": "my-surface" }
    }]);
    validate_operations(ops.as_array().unwrap()).unwrap();
}
