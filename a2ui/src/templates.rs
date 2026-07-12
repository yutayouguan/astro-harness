use serde_json::{json, Value};

use crate::catalog::ASTRO_CATALOG_ID;

/// Build A2UI operations for a confirm (approve / deny) HITL surface.
pub fn build_confirm_surface(surface_id: &str, title: &str, body: &str) -> Vec<Value> {
    vec![
        json!({
            "version": "v0.9",
            "createSurface": {
                "surfaceId": surface_id,
                "catalogId": ASTRO_CATALOG_ID
            }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    {
                        "id": "col",
                        "component": "Column",
                        "children": ["title", "body", "actions"]
                    },
                    {
                        "id": "title",
                        "component": "Text",
                        "text": title,
                        "variant": "h2"
                    },
                    {
                        "id": "body",
                        "component": "Text",
                        "text": body
                    },
                    {
                        "id": "actions",
                        "component": "Row",
                        "children": ["approve", "deny"]
                    },
                    {
                        "id": "approve",
                        "component": "Button",
                        "child": "approve_label",
                        "variant": "primary",
                        "action": { "event": { "name": "approve" } }
                    },
                    {
                        "id": "approve_label",
                        "component": "Text",
                        "text": "Approve"
                    },
                    {
                        "id": "deny",
                        "component": "Button",
                        "child": "deny_label",
                        "variant": "secondary",
                        "action": { "event": { "name": "deny" } }
                    },
                    {
                        "id": "deny_label",
                        "component": "Text",
                        "text": "Deny"
                    }
                ]
            }
        }),
    ]
}

/// Build A2UI operations for a clarify (choose among options) HITL surface.
pub fn build_clarify_surface(
    surface_id: &str,
    question: &str,
    options: &[String],
) -> Vec<Value> {
    let mut children: Vec<String> = vec!["question".into()];
    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "question",
            "component": "Text",
            "text": question,
            "variant": "h2"
        }),
    ];

    for (i, option) in options.iter().enumerate() {
        let btn_id = format!("opt_{i}");
        let label_id = format!("opt_{i}_label");
        children.push(btn_id.clone());
        components.push(json!({
            "id": btn_id,
            "component": "Button",
            "child": label_id,
            "action": {
                "event": {
                    "name": "choose",
                    "context": { "value": option }
                }
            }
        }));
        components.push(json!({
            "id": label_id,
            "component": "Text",
            "text": option
        }));
    }

    // Insert Column after root so layout order is Card → Column → children.
    components.insert(
        1,
        json!({
            "id": "col",
            "component": "Column",
            "children": children
        }),
    );

    vec![
        json!({
            "version": "v0.9",
            "createSurface": {
                "surfaceId": surface_id,
                "catalogId": ASTRO_CATALOG_ID
            }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": components
            }
        }),
    ]
}
