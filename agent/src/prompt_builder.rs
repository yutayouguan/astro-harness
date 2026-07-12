//! System prompt 分层组装器。
//!
//! 以 builder 模式将身份、静态/动态上下文、工具指引、技能索引等逐层叠入，
//! 最终 `build` 产出用 `---` 分隔的完整 system 指令字符串。

use chrono::Local;

use crate::context::{DynamicContext, StaticContext};

/// 可链式追加的 prompt 层容器。
pub struct PromptBuilder {
    /// 已接纳的各层 Markdown 片段，顺序即最终呈现顺序。
    layers: Vec<String>,
}

impl PromptBuilder {
    /// 创建不含任何层的空构建器。
    pub fn new() -> Self {
        PromptBuilder { layers: Vec::new() }
    }

    /// 追加身份层；空白内容被忽略。
    ///
    /// 参数名 `identity` 为历史命名，实际渲染为 `# 身份` 标题块。
    pub fn with_soul(mut self, identity: &str) -> Self {
        if !identity.trim().is_empty() {
            self.layers.push(format!("# 身份\n{}", identity.trim()));
        }
        self
    }

    /// 合并 `StaticContext::render` 输出为一层；渲染结果为空则跳过。
    pub fn with_static_context(mut self, ctx: &StaticContext) -> Self {
        let rendered = ctx.render();
        if !rendered.is_empty() {
            self.layers.push(rendered);
        }
        self
    }

    /// 合并 `DynamicContext::render` 输出为一层；无召回内容则跳过。
    pub fn with_dynamic_context(mut self, ctx: &DynamicContext) -> Self {
        let rendered = ctx.render();
        if !rendered.is_empty() {
            self.layers.push(rendered);
        }
        self
    }

    /// 追加工具调用格式与思考标签的使用说明（固定文案）。
    pub fn with_tool_guidance(mut self) -> Self {
        self.layers.push(
            "# 工具使用\n使用 <tool_call>{\"name\":\"...\",\"arguments\":{...}}</tool_call> 格式调用工具。\n每次思考用 <think>...</think> 标签包裹。".to_string(),
        );
        self
    }

    /// 追加长期记忆层；与 `StaticContext::memory` 标题一致，供单独注入场景使用。
    pub fn with_memory(mut self, memory_content: &str) -> Self {
        if !memory_content.is_empty() {
            self.layers
                .push(format!("# 长期记忆（MEMORY.md）\n{}", memory_content));
        }
        self
    }

    /// 追加用户画像层。
    pub fn with_user_profile(mut self, user_content: &str) -> Self {
        if !user_content.is_empty() {
            self.layers
                .push(format!("# 用户画像\n{}", user_content));
        }
        self
    }

    /// 追加当日记忆层（mermaid/日文件来源）。
    pub fn with_daily_memory(mut self, daily_content: &str) -> Self {
        if !daily_content.is_empty() {
            self.layers.push(format!(
                "# 今日记忆（mermaid/日文件）\n{}",
                daily_content
            ));
        }
        self
    }

    /// 追加可用 Skills 索引列表；`skills` 为元组 `(名称, 简述)`。
    pub fn with_skills_index(mut self, skills: &[(&str, &str)]) -> Self {
        if !skills.is_empty() {
            let index = skills
                .iter()
                .map(|(n, d)| format!("- **{}**: {}", n, d))
                .collect::<Vec<_>>()
                .join("\n");
            self.layers.push(format!("# 可用 Skills\n{}", index));
        }
        self
    }

    /// 追加 FTS/向量召回的对话上下文块（与 `DynamicContext` 标题区分，用于旧路径兼容）。
    pub fn with_recalled_context(mut self, context: &str) -> Self {
        if !context.is_empty() {
            self.layers
                .push(format!("# 召回的对话上下文\n{}", context));
        }
        self
    }

    /// 追加本地时区当前时间戳，便于模型感知「现在」。
    pub fn with_timestamp(mut self) -> Self {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S %Z");
        self.layers.push(format!("# 当前时间\n{}", now));
        self
    }

    /// 将所有层用 `\n\n---\n\n` 连接为最终 system prompt 字符串。
    pub fn build(self) -> String {
        self.layers.join("\n\n---\n\n")
    }
}

impl Default for PromptBuilder {
    /// 等价于 `PromptBuilder::new()`。
    fn default() -> Self {
        Self::new()
    }
}
