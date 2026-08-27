use serde_json::{json, Value};

use crate::catalog::ASTRO_CATALOG_ID;

/// Build A2UI operations for a confirm (approve / deny) HITL surface.
pub fn build_confirm_surface(surface_id: &str, title: &str, body: &str) -> Vec<Value> {
    build_confirm_surface_ex(surface_id, title, body, false)
}

/// Confirm surface using ClarifyWizard so it renders inline in the composer area.
///
/// A single-step wizard with the confirmation body as the question and
/// approve/deny(/always) as options. Frontend `onUiAction` maps the chosen
/// option back to `{approved: bool, always: bool}`.
pub fn build_confirm_surface_ex(
    surface_id: &str,
    title: &str,
    body: &str,
    allow_always: bool,
) -> Vec<Value> {
    let mut options = vec![
        json!("✅ Approve"),
        json!("❌ Deny"),
    ];
    if allow_always {
        options.push(json!("✅ Approve & always allow"));
    }

    let question = if title.is_empty() {
        body.to_string()
    } else {
        format!("**{title}**\n\n{body}")
    };

    let components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": ["badge", "wizard"]
        }),
        json!({
            "id": "badge",
            "component": "Badge",
            "text": "Confirm",
            "variant": "warn"
        }),
        json!({
            "id": "wizard",
            "component": "ClarifyWizard",
            "steps": [{
                "id": "confirm",
                "question": question,
                "options": options,
            }]
        }),
    ];

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

/// One step in a clarify wizard.
#[derive(Debug, Clone)]
pub struct ClarifyStep {
    /// Stable answer key (also used as tab id).
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
}

/// Clarify HITL surface via stacked-tab `ClarifyWizard` (1+ steps).
///
/// Frontend renders tabs (when 2+) + layered cards with transition animation.
/// Submit emits `choose` with `{ answers: { stepId: option }, value: summary }`.
pub fn build_clarify_surface(surface_id: &str, title: &str, steps: &[ClarifyStep]) -> Vec<Value> {
    let steps_json: Vec<Value> = steps
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "question": s.question,
                "options": s.options,
            })
        })
        .collect();

    let col_children: Vec<&str> = if steps.len() > 1 {
        vec!["badge", "title", "wizard"]
    } else {
        vec!["badge", "wizard"]
    };

    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": col_children
        }),
        json!({
            "id": "badge",
            "component": "Badge",
            "text": "Clarify",
            "variant": "info"
        }),
    ];
    if steps.len() > 1 {
        components.push(json!({
            "id": "title",
            "component": "Text",
            "text": title,
            "variant": "h2"
        }));
    }
    components.push(json!({
        "id": "wizard",
        "component": "ClarifyWizard",
        "steps": steps_json
    }));

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

/// Build A2UI operations for a location-request HITL surface (share GPS / deny / city).
pub fn build_location_request_surface(surface_id: &str, message: &str) -> Vec<Value> {
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
                        "children": ["header", "body", "actions", "city_hint", "city", "choose_city"]
                    },
                    {
                        "id": "header",
                        "component": "Row",
                        "children": ["avatar", "header_text", "badge"]
                    },
                    {
                        "id": "avatar",
                        "component": "Avatar",
                        "name": "map-pin"
                    },
                    {
                        "id": "header_text",
                        "component": "Column",
                        "children": ["title"]
                    },
                    {
                        "id": "title",
                        "component": "Text",
                        "text": "位置授权",
                        "variant": "h2"
                    },
                    {
                        "id": "badge",
                        "component": "Badge",
                        "text": "Location",
                        "variant": "info"
                    },
                    {
                        "id": "body",
                        "component": "Text",
                        "text": message
                    },
                    {
                        "id": "actions",
                        "component": "Row",
                        "children": ["share_location", "deny"]
                    },
                    {
                        "id": "share_location",
                        "component": "Button",
                        "child": "share_label",
                        "variant": "primary",
                        "action": { "event": { "name": "share_location" } }
                    },
                    {
                        "id": "share_label",
                        "component": "Text",
                        "text": "共享当前位置"
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
                        "text": "暂不分享"
                    },
                    {
                        "id": "city_hint",
                        "component": "Text",
                        "text": "或手动填写城市"
                    },
                    {
                        "id": "city",
                        "component": "TextField",
                        "label": "城市"
                    },
                    {
                        "id": "choose_city",
                        "component": "Button",
                        "child": "choose_city_label",
                        "variant": "secondary",
                        "action": { "event": { "name": "choose_city" } }
                    },
                    {
                        "id": "choose_city_label",
                        "component": "Text",
                        "text": "使用该城市"
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
    let mut children = vec!["title".to_string(), "body".to_string()];
    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "title",
            "component": "Text",
            "text": title,
            "variant": "h2"
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
        if let Some(h) = hint
            .as_ref()
            .map(|s| s.as_str().trim())
            .filter(|s| !s.is_empty())
        {
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

pub fn build_result_surface(surface_id: &str, title: &str, body: &str, status: &str) -> Vec<Value> {
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

/// Build a `deleteSurface` operation — removes an A2UI surface from the chat.
pub fn build_delete_surface(surface_id: &str) -> Vec<Value> {
    vec![json!({
        "version": "v0.9",
        "deleteSurface": { "surfaceId": surface_id }
    })]
}

/// Form field descriptor for [`build_form_surface`].
pub struct FormField<'a> {
    /// Component id (also used as data-model key).
    pub id: &'a str,
    /// `"text"` → TextField, `"checkbox"` → CheckBox.
    pub kind: &'a str,
    pub label: &'a str,
    pub required: bool,
}

/// Build a form surface with TextField / CheckBox fields and a submit button.
///
/// Emits `{ event: { name: "submit" } }` when the user taps the button.
pub fn build_form_surface(
    surface_id: &str,
    title: &str,
    fields: &[FormField<'_>],
    submit_label: &str,
) -> Vec<Value> {
    // Internal IDs are prefixed with "_" so they can never collide with caller-supplied field IDs.
    let mut col_children: Vec<String> = vec!["_title".into(), "_divider".into()];
    for f in fields {
        col_children.push(f.id.to_string());
    }
    col_children.push("_submit".into());

    let mut components = vec![
        json!({ "id": "_root", "component": "Card", "child": "_col" }),
        json!({ "id": "_col", "component": "Column", "children": col_children }),
        json!({ "id": "_title", "component": "Text", "text": title, "variant": "h2" }),
        json!({ "id": "_divider", "component": "Divider" }),
    ];

    for f in fields {
        let comp = match f.kind {
            "checkbox" => json!({
                "id": f.id,
                "component": "CheckBox",
                "label": f.label,
                "required": f.required
            }),
            _ => json!({
                "id": f.id,
                "component": "TextField",
                "label": f.label,
                "required": f.required
            }),
        };
        components.push(comp);
    }

    components.push(json!({ "id": "_spacer", "component": "Spacer" }));
    components.push(json!({
        "id": "_submit",
        "component": "Button",
        "child": "_submit_label",
        "variant": "primary",
        "action": { "event": { "name": "submit" } }
    }));
    components.push(json!({ "id": "_submit_label", "component": "Text", "text": submit_label }));

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

/// Build a network host approval surface for managed proxy denials.
///
/// Shows the target host, profile, and four action buttons:
/// allow-once, allow-for-session, allow-always (persistent), and deny.
pub fn build_network_approval_surface(
    surface_id: &str,
    host: &str,
    protocol: &str,
    port: u16,
    profile_id: &str,
    command_preview: Option<&str>,
) -> Vec<Value> {
    let target = if port == 443 || port == 80 {
        format!("{protocol}://{host}")
    } else {
        format!("{protocol}://{host}:{port}")
    };
    let body = if let Some(cmd) = command_preview {
        format!("Command `{cmd}` is trying to connect to **{target}**\n\nProfile: `{profile_id}`")
    } else {
        format!("A subprocess is trying to connect to **{target}**\n\nProfile: `{profile_id}`")
    };

    let components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": ["header", "body", "actions"]
        }),
        json!({
            "id": "header",
            "component": "Row",
            "children": ["avatar", "header_text", "badge"]
        }),
        json!({ "id": "avatar", "component": "Avatar", "name": "globe" }),
        json!({ "id": "header_text", "component": "Column", "children": ["title"] }),
        json!({ "id": "title", "component": "Text", "text": "Network Access", "variant": "h2" }),
        json!({ "id": "badge", "component": "Badge", "text": "Network", "variant": "warn" }),
        json!({ "id": "body", "component": "Text", "text": body }),
        json!({
            "id": "actions",
            "component": "Row",
            "children": ["allow_once", "allow_session", "allow_always", "deny"]
        }),
        json!({
            "id": "allow_once",
            "component": "Button",
            "child": "allow_once_label",
            "variant": "primary",
            "action": { "event": { "name": "allow_once" } }
        }),
        json!({ "id": "allow_once_label", "component": "Text", "text": "Allow once" }),
        json!({
            "id": "allow_session",
            "component": "Button",
            "child": "allow_session_label",
            "variant": "secondary",
            "action": { "event": { "name": "allow_session" } }
        }),
        json!({ "id": "allow_session_label", "component": "Text", "text": "Allow for session" }),
        json!({
            "id": "allow_always",
            "component": "Button",
            "child": "allow_always_label",
            "variant": "secondary",
            "action": { "event": { "name": "allow_always" } }
        }),
        json!({ "id": "allow_always_label", "component": "Text", "text": "Always allow" }),
        json!({
            "id": "deny",
            "component": "Button",
            "child": "deny_label",
            "variant": "secondary",
            "action": { "event": { "name": "deny" } }
        }),
        json!({ "id": "deny_label", "component": "Text", "text": "Deny" }),
    ];

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

/// Build a tag/chip-list surface — title row + horizontal row of Chip labels.
pub fn build_chip_list_surface(
    surface_id: &str,
    title: &str,
    chips: &[impl AsRef<str>],
) -> Vec<Value> {
    let chip_ids: Vec<String> = chips
        .iter()
        .enumerate()
        .map(|(i, _)| format!("chip{i}"))
        .collect();

    let col_children = if chips.is_empty() {
        vec!["title".to_string()]
    } else {
        vec!["title".to_string(), "row".to_string()]
    };

    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({ "id": "col", "component": "Column", "children": col_children }),
        json!({ "id": "title", "component": "Text", "text": title, "variant": "h2" }),
    ];

    if !chips.is_empty() {
        components.push(json!({ "id": "row", "component": "Row", "children": chip_ids }));
        for (i, chip) in chips.iter().enumerate() {
            let label = chip.as_ref();
            components
                .push(json!({ "id": format!("chip{i}"), "component": "Chip", "text": label }));
        }
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
