use a2ui::templates::{build_clarify_surface, build_confirm_surface};
use a2ui::validate_operations;

#[test]
fn confirm_template_validates() {
    let ops = build_confirm_surface(
        "surf-confirm-1",
        "删除文件？",
        "将永久删除 report.pdf",
    );
    validate_operations(&ops).unwrap();
}

#[test]
fn clarify_template_validates() {
    let ops = build_clarify_surface(
        "surf-clarify-1",
        "选哪个环境？",
        &["staging".into(), "production".into()],
    );
    validate_operations(&ops).unwrap();
}
