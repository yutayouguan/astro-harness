use serde_json::{json, Value};

use crate::catalog::ASTRO_CATALOG_ID;

/// 构建确认（approve / deny）HITL 表面的 A2UI 操作。
pub fn build_confirm_surface(surface_id: &str, title: &str, body: &str) -> Vec<Value> {
    build_confirm_surface_ex(surface_id, title, body, false)
}

/// 使用 ClarifyWizard 的 approval 变体构建确认表面，使其在
/// composer 区域内联渲染，而不会将结构化内容展平为类 markdown 的问题。
pub fn build_confirm_surface_ex(
    surface_id: &str,
    title: &str,
    body: &str,
    allow_always: bool,
) -> Vec<Value> {
    build_confirm_surface_with_rule(surface_id, title, body, allow_always, None)
}

/// 带可选低风险命令族规则的审批确认表面。
pub fn build_confirm_surface_with_rule(
    surface_id: &str,
    title: &str,
    body: &str,
    allow_always: bool,
    command_family: Option<&str>,
) -> Vec<Value> {
    let mut options = vec![json!("approve"), json!("deny")];
    if allow_always {
        options.push(json!("approve_always"));
    }
    if command_family.is_some() {
        options.push(json!("approve_type"));
    }
    let question = if title.is_empty() { body } else { title };

    let components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": ["wizard"]
        }),
        json!({
            "id": "wizard",
            "component": "ClarifyWizard",
            "variant": "approval",
            "title": title,
            "body": body,
            "allowAlways": allow_always,
            "approvalTypeLabel": command_family,
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

/// 沙箱重试审批表面。面向用户的文案由前端 locale 解析；
/// 后端仅发送语义类型和原始拒绝详情。
pub fn build_sandbox_retry_surface(surface_id: &str, denial_detail: &str) -> Vec<Value> {
    let components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": ["wizard"]
        }),
        json!({
            "id": "wizard",
            "component": "ClarifyWizard",
            "variant": "approval",
            "approvalKind": "sandbox_retry",
            "approvalDetail": denial_detail,
            "steps": [{
                "id": "confirm",
                "question": "sandbox_retry",
                "options": ["approve", "deny"],
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

/// 澄清向导中的单个步骤。
#[derive(Debug, Clone)]
pub struct ClarifyStep {
    /// 稳定的应答键（也用作 tab id）。
    pub id: String,
    pub question: String,
    pub options: Vec<String>,
}

/// 通过堆叠标签页 `ClarifyWizard`（1 个或多个步骤）的澄清 HITL 表面。
///
/// 前端渲染标签页（2 个以上时）+ 带过渡动画的分层卡片。
/// 提交时发出 `choose` 事件，携带 `{ answers: { stepId: option }, value: summary }`。
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

/// 构建位置请求 HITL 表面的 A2UI 操作（共享 GPS / 拒绝 / 手动填写城市）。
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

/// 构建只读信息卡片表面（标题 + 正文 + 可选图片）。
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

/// 玻璃 Card 内的指标列表（标题 + Metric 行）。
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

/// 构建 `deleteSurface` 操作 -- 从聊天中移除一个 A2UI 表面。
pub fn build_delete_surface(surface_id: &str) -> Vec<Value> {
    vec![json!({
        "version": "v0.9",
        "deleteSurface": { "surfaceId": surface_id }
    })]
}

/// [`build_form_surface`] 的表单字段描述符。
pub struct FormField<'a> {
    /// 组件 id（也用作数据模型键）。
    pub id: &'a str,
    /// `"text"` 对应 TextField，`"checkbox"` 对应 CheckBox。
    pub kind: &'a str,
    pub label: &'a str,
    pub required: bool,
}

/// 构建包含 TextField / CheckBox 字段和提交按钮的表单表面。
///
/// 用户点击按钮时发出 `{ event: { name: "submit" } }`。
pub fn build_form_surface(
    surface_id: &str,
    title: &str,
    fields: &[FormField<'_>],
    submit_label: &str,
) -> Vec<Value> {
    // 内部 ID 以 "_" 为前缀，确保不会与调用方提供的字段 ID 冲突。
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

/// 构建受管代理拒绝的网络主机审批表面。
///
/// 显示目标主机、profile 以及四个操作按钮：
/// 允许一次、允许本次会话、始终允许（持久化）和拒绝。
/// 网络主机审批表面。
///
/// 与 confirm / sandbox_retry 共用 `ClarifyWizard` 的 approval 变体：面向用户的文案
/// 由前端 locale 按 `approvalKind = "network"` 解析，后端只发送语义类型与原始目标
/// （host / profile / 命令预览）。动作沿用 `allow_once` / `allow_session` /
/// `allow_always` / `deny`，与 `park_network_approval` 的 `scope` 契约一致。
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

    let components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({
            "id": "col",
            "component": "Column",
            "children": ["wizard"]
        }),
        json!({
            "id": "wizard",
            "component": "ClarifyWizard",
            "variant": "approval",
            "approvalKind": "network",
            "approvalDetail": target,
            "approvalHost": host,
            "approvalProfile": profile_id,
            "approvalCommand": command_preview,
            "allowSession": true,
            "allowAlways": true,
            "approvalTypeLabel": host,
            "steps": [{
                "id": "network",
                "question": "network_approval",
                "options": ["allow_once", "allow_session", "allow_always", "deny"],
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

/// 构建标签/芯片列表表面 -- 标题行 + 水平排列的 Chip 标签行。
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
