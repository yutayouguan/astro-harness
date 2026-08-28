use a2ui::templates::{
    build_clarify_surface, build_confirm_surface, build_info_surface,
    build_location_request_surface, ClarifyStep,
};
use a2ui::{validate_operations, ASTRO_CATALOG_ID};
use serde_json::Value;

fn catalog_ids(ops: &[Value]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| {
            op.get("createSurface")
                .and_then(|c| c.get("catalogId"))
                .and_then(|v| v.as_str())
        })
        .collect()
}

fn all_component_names(ops: &[Value]) -> Vec<String> {
    let mut names = Vec::new();
    for op in ops {
        if let Some(comps) = op
            .pointer("/updateComponents/components")
            .and_then(|v| v.as_array())
        {
            for c in comps {
                if let Some(n) = c.get("component").and_then(|v| v.as_str()) {
                    names.push(n.to_string());
                }
            }
        }
    }
    names
}

fn wizard_step_count(ops: &[Value]) -> usize {
    ops.iter()
        .find_map(|op| {
            op.pointer("/updateComponents/components")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter().find(|c| {
                        c.get("component").and_then(|n| n.as_str()) == Some("ClarifyWizard")
                    })
                })
                .and_then(|w| w.get("steps"))
                .and_then(|v| v.as_array())
                .map(|a| a.len())
        })
        .unwrap_or(0)
}

#[test]
fn confirm_template_validates_and_uses_v2() {
    let ops = build_confirm_surface("surf-confirm-1", "删除文件？", "将永久删除 report.pdf");
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "ClarifyWizard"));
    let wizard = ops
        .iter()
        .find_map(|op| {
            op.pointer("/updateComponents/components")
                .and_then(Value::as_array)
        })
        .and_then(|components| {
            components.iter().find(|component| {
                component.get("component").and_then(Value::as_str) == Some("ClarifyWizard")
            })
        })
        .expect("confirm wizard");
    assert_eq!(
        wizard.get("variant").and_then(Value::as_str),
        Some("approval")
    );
    assert_eq!(
        wizard.get("title").and_then(Value::as_str),
        Some("删除文件？")
    );
    assert_eq!(
        wizard.get("body").and_then(Value::as_str),
        Some("将永久删除 report.pdf")
    );
    assert_eq!(
        wizard.get("allowAlways").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn confirm_template_exposes_persistent_approval_without_ui_copy_in_protocol() {
    let ops = a2ui::templates::build_confirm_surface_ex(
        "surf-confirm-always",
        "批准危险命令",
        "检测到潜在危险操作\n\n```\necho ok\n```",
        true,
    );
    let wizard = ops
        .iter()
        .find_map(|op| {
            op.pointer("/updateComponents/components")
                .and_then(Value::as_array)
        })
        .and_then(|components| components.iter().find(|c| c["id"] == "wizard"))
        .expect("confirm wizard");
    assert_eq!(
        wizard.get("allowAlways").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        wizard.pointer("/steps/0/options"),
        Some(&serde_json::json!(["approve", "deny", "approve_always"]))
    );
}

#[test]
fn clarify_single_step_uses_wizard() {
    let steps = [ClarifyStep {
        id: "env".into(),
        question: "选哪个环境？".into(),
        options: vec!["staging".into(), "production".into()],
    }];
    let ops = build_clarify_surface("surf-clarify-1", "选哪个环境？", &steps);
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "ClarifyWizard"));
    assert!(!names.iter().any(|n| n == "ChoicePicker"));
    assert_eq!(wizard_step_count(&ops), 1);
    let has_title_id = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|c| c.get("id").and_then(|i| i.as_str()) == Some("title"))
            })
            .unwrap_or(false)
    });
    assert!(!has_title_id);
}

#[test]
fn clarify_multi_step_template_validates() {
    let steps = [
        ClarifyStep {
            id: "style".into(),
            question: "风格偏好？".into(),
            options: vec!["民谣".into(), "电子".into()],
        },
        ClarifyStep {
            id: "lyrics".into(),
            question: "歌词？".into(),
            options: vec!["你写".into(), "纯音乐".into()],
        },
        ClarifyStep {
            id: "mood".into(),
            question: "氛围？".into(),
            options: vec!["欢快洗脑".into(), "优美自然".into()],
        },
    ];
    let ops = build_clarify_surface("surf-clarify-multi", "开干前确认", &steps);
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "ClarifyWizard"));
    assert_eq!(wizard_step_count(&ops), 3);
    let has_title_id = ops.iter().any(|op| {
        op.pointer("/updateComponents/components")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .any(|c| c.get("id").and_then(|i| i.as_str()) == Some("title"))
            })
            .unwrap_or(false)
    });
    assert!(has_title_id);
}

#[test]
fn location_request_template_validates() {
    let ops = build_location_request_surface("surf-loc-1", "需要定位以查询天气");
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "TextField"));
    assert!(names.iter().any(|n| n == "Button"));
    assert!(names.iter().any(|n| n == "Avatar"));
}

#[test]
fn info_template_validates_with_optional_image() {
    let ops = build_info_surface(
        "surf-info-1",
        "部署摘要",
        "3 服务已更新",
        Some("https://example.com/a.png"),
    );
    validate_operations(&ops).unwrap();
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "Image"));
    assert!(!names.iter().any(|n| n == "Badge"));
}
