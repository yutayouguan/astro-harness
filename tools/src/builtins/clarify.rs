//! 澄清工具：任务含糊时向用户提出确认问题。
//!
//! 不执行副作用，仅把问题包装为 `<clarify>` 标记文本，由 UI / Agent 层展示给用户。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `clarify` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyArgs {
    /// 向用户提出的澄清问题（不可为空）。
    pub question: String,
}

/// 向注册表登记 `clarify` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "clarify".to_string(),
        toolset: "clarify".to_string(),
        description: "Ask the user a clarifying question before proceeding with an ambiguous task."
            .to_string(),
        schema: schema_for_args::<ClarifyArgs>(),
        check_fn: None,
        icon: "circle-help",
    });
}

/// 校验问题非空，并返回带 `<clarify>` 标签的提示文本。
///
/// # 参数
/// - `_ctx`：未使用（保持与其他工具统一签名）
/// - `args`：JSON，需符合 [`ClarifyArgs`]
///
/// # 返回
/// 成功时为 `\<clarify\>…\</clarify\>` 格式字符串。
///
/// # 错误
/// 参数无法反序列化，或 `question` 为空。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ClarifyArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("clarify 参数无效: {e}"))?;
    let question = parsed.question.trim();
    if question.is_empty() {
        anyhow::bail!("clarify 需要 question");
    }
    Ok(format!("<clarify>\n{question}\n</clarify>"))
}
