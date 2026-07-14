use a2ui::templates::{
    build_callout_surface, build_chip_list_surface, build_delete_surface, build_form_surface,
    build_metrics_surface, build_result_surface, FormField,
};
use a2ui::validate_operations;

#[test]
fn metrics_recipe_validates() {
    let ops = build_metrics_surface(
        "surf-m1",
        "系统指标",
        &[
            ("CPU".into(), "42%".into(), Some("正常".into())),
            ("内存".into(), "1.2G".into(), None),
        ],
    );
    validate_operations(&ops).unwrap();
}

#[test]
fn callout_recipe_validates() {
    let ops = build_callout_surface("surf-c1", "注意", "即将重启", "warn");
    validate_operations(&ops).unwrap();
}

#[test]
fn result_recipe_validates() {
    let ops = build_result_surface("surf-r1", "完成", "部署成功", "success");
    validate_operations(&ops).unwrap();
}

#[test]
fn delete_surface_validates() {
    let ops = build_delete_surface("surf-to-remove");
    validate_operations(&ops).unwrap();
    assert_eq!(ops.len(), 1);
    assert!(ops[0].get("deleteSurface").is_some());
}

#[test]
fn form_surface_validates() {
    let fields = [
        FormField { id: "name", kind: "text", label: "姓名", required: true },
        FormField { id: "agree", kind: "checkbox", label: "同意条款", required: false },
    ];
    let ops = build_form_surface("surf-form1", "提交信息", &fields, "确认提交");
    validate_operations(&ops).unwrap();
}

#[test]
fn chip_list_surface_validates() {
    let chips = vec!["Rust".into(), "A2UI".into(), "Astro".into()];
    let ops = build_chip_list_surface("surf-chips1", "技术栈", &chips);
    validate_operations(&ops).unwrap();
}

#[test]
fn chip_list_empty_validates() {
    let ops = build_chip_list_surface("surf-chips-empty", "标签", &[]);
    validate_operations(&ops).unwrap();
}
