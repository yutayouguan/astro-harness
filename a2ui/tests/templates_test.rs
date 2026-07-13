use a2ui::templates::{build_clarify_surface, build_confirm_surface, build_info_surface};
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

#[test]
fn confirm_template_validates_and_uses_v2() {
    let ops = build_confirm_surface("surf-confirm-1", "删除文件？", "将永久删除 report.pdf");
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "Avatar" || n == "Badge"));
    assert!(names.iter().any(|n| n == "Button"));
}

#[test]
fn clarify_template_validates() {
    let ops = build_clarify_surface(
        "surf-clarify-1",
        "选哪个环境？",
        &["staging".into(), "production".into()],
    );
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
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
}
