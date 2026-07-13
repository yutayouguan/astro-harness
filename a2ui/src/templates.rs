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
                        "children": ["header", "body", "actions"]
                    },
                    {
                        "id": "header",
                        "component": "Row",
                        "children": ["avatar", "header_text", "badge"]
                    },
                    {
                        "id": "avatar",
                        "component": "Avatar",
                        "name": "shield"
                    },
                    {
                        "id": "header_text",
                        "component": "Column",
                        "children": ["title"]
                    },
                    {
                        "id": "title",
                        "component": "Text",
                        "text": title,
                        "variant": "h2"
                    },
                    {
                        "id": "badge",
                        "component": "Badge",
                        "text": "Confirm",
                        "variant": "warn"
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
    let option_values: Vec<Value> = options.iter().map(|o| json!(o)).collect();
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
                        "children": ["badge", "question", "value", "submit"]
                    },
                    {
                        "id": "badge",
                        "component": "Badge",
                        "text": "Clarify",
                        "variant": "info"
                    },
                    {
                        "id": "question",
                        "component": "Text",
                        "text": question,
                        "variant": "h2"
                    },
                    {
                        "id": "value",
                        "component": "ChoicePicker",
                        "label": "选项",
                        "options": option_values,
                        "required": true
                    },
                    {
                        "id": "submit",
                        "component": "Button",
                        "child": "submit_label",
                        "variant": "primary",
                        "action": { "event": { "name": "choose" } }
                    },
                    {
                        "id": "submit_label",
                        "component": "Text",
                        "text": "Submit"
                    }
                ]
            }
        }),
    ]
}

/// Build a read-only info card surface (title + body + optional image).
pub fn build_info_surface(
    surface_id: &str,
    title: &str,
    body: &str,
    image_url: Option<&str>,
) -> Vec<Value> {
    let mut children = vec!["title".to_string(), "badge".to_string(), "body".to_string()];
    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "title",
            "component": "Text",
            "text": title,
            "variant": "h2"
        }),
        json!({
            "id": "badge",
            "component": "Badge",
            "text": "Info",
            "variant": "info"
        }),
        json!({
            "id": "body",
            "component": "Text",
            "text": body
        }),
    ];
    if let Some(url) = image_url.map(str::trim).filter(|u| !u.is_empty()) {
        children.push("img".into());
        components.push(json!({
            "id": "img",
            "component": "Image",
            "url": url
        }));
    }
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

/// Metrics list inside a glass Card (title + Metric rows).
pub fn build_metrics_surface(
    surface_id: &str,
    title: &str,
    metrics: &[(String, String, Option<String>)],
) -> Vec<Value> {
    let mut col_children = vec!["title".to_string()];
    for (i, _) in metrics.iter().enumerate() {
        col_children.push(format!("m{i}"));
    }
    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({ "id": "col", "component": "Column", "children": col_children }),
        json!({ "id": "title", "component": "Text", "text": title, "variant": "h2" }),
    ];
    for (i, (label, value, hint)) in metrics.iter().enumerate() {
        let mut m = json!({
            "id": format!("m{i}"),
            "component": "Metric",
            "label": label,
            "value": value
        });
        if let Some(h) = hint.as_ref().map(|s| s.as_str().trim()).filter(|s| !s.is_empty()) {
            m["hint"] = json!(h);
        }
        components.push(m);
    }
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": { "surfaceId": surface_id, "components": components }
        }),
    ]
}

pub fn build_callout_surface(
    surface_id: &str,
    title: &str,
    body: &str,
    variant: &str,
) -> Vec<Value> {
    let v = match variant {
        "warn" | "info" => variant,
        _ => "info",
    };
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["title", "callout"] },
                    { "id": "title", "component": "Text", "text": title, "variant": "h2" },
                    { "id": "callout", "component": "Callout", "text": body, "variant": v }
                ]
            }
        }),
    ]
}

pub fn build_result_surface(
    surface_id: &str,
    title: &str,
    body: &str,
    status: &str,
) -> Vec<Value> {
    let v = match status {
        "warn" | "danger" | "success" | "info" => status,
        _ => "success",
    };
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["header", "body"] },
                    { "id": "header", "component": "Row", "children": ["title", "badge"] },
                    { "id": "title", "component": "Text", "text": title, "variant": "h2" },
                    { "id": "badge", "component": "Badge", "text": v, "variant": v },
                    { "id": "body", "component": "Text", "text": body }
                ]
            }
        }),
    ]
}
