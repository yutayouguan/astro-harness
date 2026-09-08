//! Prompt 文本片段组装器。
//!
//! 以 builder 模式生成可组合的身份、工具指引和技能索引片段。
//! 真实采样边界由 `PromptContract` 负责，不应把动态上下文或工具 schema
//! 压平到基础指令中。

use chrono::Local;

use crate::prompt::context::{DynamicContext, StaticContext};

/// 工具调用与学习闭环固定指引（注入稳定基础指令）。
pub const TOOL_GUIDANCE: &str = concat!(include_str!("behavior_guidance.md"), "\n", "\
# 工具使用\n\
调用工具时只使用模型提供的原生结构化工具接口；不得把工具名称和参数 JSON 写入回答正文。\n\
加载 Skill 时工具名必须是 skills，arguments.skill_id 填 Skill 名称；可用 action=list|curate|load|manage。\n\
复杂可复用流程：skills manage create；纠错后的正确步骤：manage_action=patch（old_string 须唯一）。\n\
长期偏好/环境事实：用 memory；跨会话原文：context_search（scope=session 或 all）。闲置技能：action=curate（只建议，确认后再 delete）。\n\
用户明确要求查看、测试或操作网页时使用 browser_open；启动本地开发服务器并获得 loopback URL 后，自动 browser_open，再用 browser_snapshot/click/type/scroll/wait/screenshot 验证修改。仅为读取链接内容时优先 web_fetch，不要无故弹出网页预览。浏览器表单提交、发布、删除、登录、权限或购买操作必须声明 state_changing/sensitive intent。\n\
需要澄清时使用 ask_user（mode=question，questions）；需要操作确认时使用 ask_user（mode=confirm，title+body）；本地天气/附近定位用 ask_user（mode=location）。Agent↔Plan 切换只用 switch_mode（勿与 ask_user 混用）。\n\
Agent Thread 只使用六个 V2 工具：spawn_agent(task_name,message,agent_type?,model?,reasoning_effort?,fork_turns?)；list_agents(path_prefix?)；send_message(target,message) 仅排队；followup_task(target,message) 排队并触发下一轮；wait_agent(timeout_ms?)；interrupt_agent(target)。新建可切换的长期助手才用 persona_create。\n\
向用户展示本工作区媒体/网页时，在回复正文写 ![audio](path) / ![video](path) / ![image](path) / ![html](path)；path 用工具返回的工作区相对路径（如 generated/audio/…、generated/html/…），HTML 文件请写入 generated/html/ 目录；不要写绝对路径，也不要用「文件：`路径`」这类纯文本，更不要用 present / A2UI 挂媒体卡。\n\
展示已写入的代码/文本文件（.py/.rs/.c/.ts/.json/.md 等）时，同样在正文写 ![code](path) 引用工作区相对路径，前端会按后缀语法高亮渲染成可复制/下载/引用的代码卡片；不要把文件全文再粘回正文，避免重复占用上下文。\n\
推理内容使用模型原生 reasoning 通道，不要在回答正文中输出隐藏思考标记。");

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

    /// 追加工具调用格式与学习闭环说明（固定文案）。
    pub fn with_tool_guidance(mut self) -> Self {
        self.layers.push(TOOL_GUIDANCE.to_string());
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
            self.layers.push(format!("# 用户画像\n{}", user_content));
        }
        self
    }

    /// 追加当日记忆层（`memory/YYYY-MM-DD.md`）。
    pub fn with_daily_memory(mut self, daily_content: &str) -> Self {
        if !daily_content.is_empty() {
            self.layers
                .push(format!("# 今日记忆（流水截断）\n{}", daily_content));
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
            self.layers.push(format!(
                "# 可用 Skills\n\
                 通过工具 `skills` 加载（arguments.skill_id = 下列名称），不要把 Skill 名当作工具名直接调用。\n\
                 {index}"
            ));
        }
        self
    }

    /// 追加 FTS/向量召回的对话上下文块（标题与 `DynamicContext` 区分）。
    pub fn with_recalled_context(mut self, context: &str) -> Self {
        if !context.is_empty() {
            self.layers.push(format!("# 召回的对话上下文\n{}", context));
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

#[cfg(test)]
mod tests {
    use super::TOOL_GUIDANCE;

    #[test]
    fn fixed_guidance_keeps_contracts_not_customizable_workflow() {
        let base = include_str!("behavior_guidance.md");
        assert!(base.contains("问答、评估和诊断不自动授权"));
        assert!(base.contains("notes_stale=true"));
        assert!(base.contains("compaction_status"));
        assert!(!base.contains("结论先行"));
        assert!(!base.contains("探索 → 计划"));
        assert!(!base.contains("匹配用户的语气"));
        assert!(TOOL_GUIDANCE.contains("mode=question"));
    }

    #[test]
    fn tool_guidance_exposes_only_six_v2_agent_thread_tools() {
        for name in [
            "spawn_agent",
            "list_agents",
            "send_message",
            "followup_task",
            "wait_agent",
            "interrupt_agent",
        ] {
            assert!(TOOL_GUIDANCE.contains(name), "missing {name}");
        }
        for legacy in [
            "read_agent",
            "close_agent",
            "send_message_to_agent",
            "wait_agents",
        ] {
            assert!(!TOOL_GUIDANCE.contains(legacy), "legacy {legacy} leaked");
        }
        assert!(TOOL_GUIDANCE.contains("task_name"));
        assert!(TOOL_GUIDANCE.contains("message"));
        assert!(TOOL_GUIDANCE.contains("target"));
        assert!(!TOOL_GUIDANCE.contains("arguments.task"));
        assert!(!TOOL_GUIDANCE.contains("thread_id"));
        assert!(!TOOL_GUIDANCE.contains("thread_ids"));
        assert!(TOOL_GUIDANCE.contains("原生结构化工具接口"));
        assert!(!TOOL_GUIDANCE.contains("<tool_call>"));
        assert!(!TOOL_GUIDANCE.contains("<think>"));
    }
}
