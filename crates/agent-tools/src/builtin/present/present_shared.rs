//! 共享 dispatch 辅助函数，用于 present_metrics / present_callout / present_result。

use serde_json::{json, Value};

/// 验证 A2UI operations，包装错误信息后返回 JSON 字符串。
///
/// - `ops_fn`：接受 `surface_id` 并返回 operations 数组
/// - `summary`：写入结果 `"summary"` 字段的文本
pub fn dispatch_present(
    tool_name: &str,
    summary: &str,
    ops_fn: impl FnOnce(&str) -> Vec<Value>,
) -> anyhow::Result<String> {
    let surface_id = format!("{}-{}", tool_name, uuid::Uuid::new_v4());
    let ops = ops_fn(&surface_id);
    a2ui::validate_operations(&ops).map_err(|e| anyhow::anyhow!("{tool_name} A2UI 无效: {e}"))?;
    Ok(json!({
        "astro_ui": true,
        "summary": summary,
        "operations": ops,
    })
    .to_string())
}
