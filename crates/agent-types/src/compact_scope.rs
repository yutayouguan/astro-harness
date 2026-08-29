use serde::{Deserialize, Serialize};

/// 控制压缩 token 限额的计量方式。
///
/// - `Total`：限额作用于整个对话（默认）。
/// - `BodyAfterPrefix`：限额仅作用于 system prompt 前缀之后的正文，
///   使前缀不计入预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompactTokenLimitScope {
    #[default]
    Total,
    BodyAfterPrefix,
}
