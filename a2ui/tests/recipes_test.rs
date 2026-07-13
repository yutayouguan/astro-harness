use a2ui::templates::{
    build_callout_surface, build_metrics_surface, build_result_surface,
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
