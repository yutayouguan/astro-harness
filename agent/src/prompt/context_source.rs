//! 可插拔上下文源 + 字符预算（对齐 Agno ContextProvider 思路）。
//!
//! 各层通过 [`ContextSource::contribute`] 在共享 [`ContextBudget`] 下取字符；
//! 优先级由调用方排列：static → inject → skills → guidance/timestamp → dynamic。

use crate::prompt::context::{DynamicContext, StaticContext};

/// System prompt 字符预算（按 Unicode 标量计）。
#[derive(Debug, Clone)]
pub struct ContextBudget {
    remaining: usize,
}

impl ContextBudget {
    pub fn new(max_chars: usize) -> Self {
        Self {
            remaining: max_chars,
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// 把先前扣掉的字符数还回预算（用于预留分隔符后本层实际为空的回滚）。
    pub fn refund(&mut self, n: usize) {
        self.remaining = self.remaining.saturating_add(n);
    }

    /// 从预算中取出尽可能多的字符；预算耗尽时返回空串。
    pub fn take_chars(&mut self, s: &str) -> String {
        if self.remaining == 0 || s.is_empty() {
            return String::new();
        }
        let n = s.chars().count();
        if n <= self.remaining {
            self.remaining -= n;
            s.to_string()
        } else {
            let out: String = s.chars().take(self.remaining).collect();
            self.remaining = 0;
            out
        }
    }
}

/// 统一上下文源协议。
pub trait ContextSource {
    fn id(&self) -> &str;
    fn contribute(&self, budget: &mut ContextBudget) -> String;
}

/// 包装已渲染字符串的简单源。
pub struct RenderedSource {
    id: &'static str,
    body: String,
}

impl RenderedSource {
    pub fn new(id: &'static str, body: impl Into<String>) -> Self {
        Self {
            id,
            body: body.into(),
        }
    }
}

impl ContextSource for RenderedSource {
    fn id(&self) -> &str {
        self.id
    }

    fn contribute(&self, budget: &mut ContextBudget) -> String {
        let t = self.body.trim();
        if t.is_empty() {
            return String::new();
        }
        budget.take_chars(t)
    }
}

impl ContextSource for StaticContext {
    fn id(&self) -> &str {
        "static"
    }

    fn contribute(&self, budget: &mut ContextBudget) -> String {
        budget.take_chars(&self.render())
    }
}

impl ContextSource for DynamicContext {
    fn id(&self) -> &str {
        "dynamic"
    }

    fn contribute(&self, budget: &mut ContextBudget) -> String {
        budget.take_chars(&self.render())
    }
}

const LAYER_SEP: &str = "\n\n---\n\n";

/// 按优先级组装 system prompt；预算优先留给靠前的源。
///
/// 在已有层上追加时，先预留 [`LAYER_SEP`] 再 `contribute`，避免极限预算下
/// 内容已扣费却接不上分隔符、或分隔符挤占后续层的不精确行为。
pub fn assemble_from_sources(
    budget: &mut ContextBudget,
    sources: &[&dyn ContextSource],
) -> String {
    let sep_cost = LAYER_SEP.chars().count();
    let mut layers = Vec::new();
    for src in sources {
        if !layers.is_empty() {
            // 需要分隔符 + 至少 1 个内容字符，否则本层及之后都放不下
            if budget.remaining() < sep_cost + 1 {
                break;
            }
            let _ = budget.take_chars(LAYER_SEP);
        }
        let chunk = src.contribute(budget);
        if chunk.is_empty() {
            // 预留了分隔符却没有内容：把分隔符预算还回去（语义上本层未加入）
            if !layers.is_empty() {
                budget.refund(sep_cost);
            }
            continue;
        }
        layers.push(chunk);
    }
    layers.join(LAYER_SEP)
}

/// 默认最大 system prompt 字符（桌面场景宽裕上限）。
pub const DEFAULT_CONTEXT_BUDGET_CHARS: usize = 200_000;

/// 标准 Astro 层序组装：static → inject → skills → guidance → timestamp → dynamic。
pub fn assemble_system_layers(
    budget: &mut ContextBudget,
    static_ctx: &StaticContext,
    inject: Option<&str>,
    skill_index: &[(&str, &str)],
    dynamic_ctx: &DynamicContext,
    guidance: &str,
    timestamp: &str,
) -> String {
    let inject_body = inject
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| format!("# 临时注入上下文\n{s}"))
        .unwrap_or_default();
    let inject_src = RenderedSource::new("inject", inject_body);

    let skills_body = if skill_index.is_empty() {
        String::new()
    } else {
        let index = skill_index
            .iter()
            .map(|(n, d)| format!("- **{n}**: {d}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "# 可用 Skills\n\
             通过工具 `skills` 加载（arguments.skill_id = 下列名称），不要把 Skill 名当作工具名直接调用。\n\
             {index}"
        )
    };
    let skills_src = RenderedSource::new("skills", skills_body);
    let guidance_src = RenderedSource::new("guidance", guidance);
    let ts_src = RenderedSource::new("timestamp", timestamp);

    assemble_from_sources(
        budget,
        &[
            static_ctx,
            &inject_src,
            &skills_src,
            &guidance_src,
            &ts_src,
            dynamic_ctx,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_preserved_when_budget_tight() {
        let static_ctx = StaticContext {
            soul: "SOUL_CONTENT_ABCDEF".into(),
            ..Default::default()
        };
        let dynamic = DynamicContext::from_recalled(3, "DYNAMIC_SHOULD_BE_CUT_OR_DROPPED");
        let mut budget = ContextBudget::new(static_ctx.render().chars().count() + 20);
        let out = assemble_system_layers(
            &mut budget,
            &static_ctx,
            None,
            &[],
            &dynamic,
            "# 工具使用\nguidance",
            "# 当前时间\nnow",
        );
        assert!(out.contains("SOUL_CONTENT_ABCDEF"));
        assert!(
            !out.contains("DYNAMIC_SHOULD_BE_CUT_OR_DROPPED")
                || out.find("SOUL_CONTENT_ABCDEF").unwrap()
                    < out.find("DYNAMIC").unwrap_or(usize::MAX)
        );
    }

    #[test]
    fn dynamic_dropped_when_no_budget_left() {
        let static_ctx = StaticContext {
            soul: "AAAA".into(),
            ..Default::default()
        };
        let dynamic = DynamicContext::from_recalled(1, "BBBB_DYNAMIC");
        let mut budget = ContextBudget::new(static_ctx.render().chars().count());
        let out = assemble_from_sources(&mut budget, &[&static_ctx, &dynamic]);
        assert!(out.contains("AAAA"));
        assert!(!out.contains("BBBB_DYNAMIC"));
        assert_eq!(budget.remaining(), 0);
    }

    #[test]
    fn separator_reserved_before_next_layer() {
        // 第一层 4 字；sep=7；第二层至少还要 1 字 → 预算 4+7=11 时第二层应被丢弃
        let a = RenderedSource::new("a", "AAAA");
        let b = RenderedSource::new("b", "BBBB");
        let mut budget = ContextBudget::new(4 + LAYER_SEP.chars().count());
        let out = assemble_from_sources(&mut budget, &[&a, &b]);
        assert_eq!(out, "AAAA");
        assert!(!out.contains("BBBB"));
        // 预留 sep 后 contribute 为空会 refund，或不足 sep+1 则直接 break
        assert!(budget.remaining() < LAYER_SEP.chars().count() + 1);
    }

    #[test]
    fn empty_middle_layer_does_not_consume_sep() {
        let a = RenderedSource::new("a", "AAAA");
        let empty = RenderedSource::new("empty", "   ");
        let b = RenderedSource::new("b", "BBBB");
        let mut budget = ContextBudget::new(4 + LAYER_SEP.chars().count() + 4);
        let out = assemble_from_sources(&mut budget, &[&a, &empty, &b]);
        assert_eq!(out, format!("AAAA{LAYER_SEP}BBBB"));
        assert_eq!(budget.remaining(), 0);
    }
}
