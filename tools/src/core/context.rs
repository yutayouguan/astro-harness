//! 工具执行上下文：由 AgentLoop 注入的运行时依赖与凭证。
//!
//! 各内置工具的 `dispatch` 函数通过 [`ToolContext`] 访问工作区路径、
//! 记忆管理器、Provider 注册表及聊天/生图凭证，避免在工具层重复读取环境变量。

use std::path::PathBuf;
use std::sync::Arc;

use memory::MemoryManager;
use providers::registry::ProviderRegistry;

/// 单个图片生成 Provider 的调用凭证。
///
/// 由 Tauri 前端从「模型提供商」面板注入，经 [`ImageGenTargets`] 传给 `image_gen` 工具。
#[derive(Debug, Clone, Default)]
pub struct ImageGenCreds {
    /// Provider id，如 `"google"`、`"openai"`。
    pub provider: String,
    /// 图片模型名称；为空时由 `providers::image_gen::default_image_model` 补全。
    pub model: String,
    /// API Key。
    pub api_key: String,
    /// 自定义 Base URL；为空时使用 Provider 默认值。
    pub base_url: String,
}

/// 主备图片生成凭证对：主 Provider 失败时自动尝试备用。
#[derive(Debug, Clone, Default)]
pub struct ImageGenTargets {
    /// 优先使用的图片生成凭证。
    pub primary: Option<ImageGenCreds>,
    /// 主 Provider 失败时的备用凭证。
    pub fallback: Option<ImageGenCreds>,
}

impl ImageGenTargets {
    /// 从 Tauri 面板传入的字符串字段构建主备凭证。
    ///
    /// Provider 或 API Key 为空时，对应槽位为 `None`；model 为空则使用 Provider 默认图片模型。
    pub fn from_parts(
        provider: &str,
        model: &str,
        api_key: &str,
        base_url: &str,
        fb_provider: &str,
        fb_model: &str,
        fb_api_key: &str,
        fb_base_url: &str,
    ) -> Self {
        let primary = if !provider.is_empty() && !api_key.is_empty() {
            Some(ImageGenCreds {
                provider: provider.to_string(),
                model: if model.is_empty() {
                    providers::image_gen::default_image_model(provider).to_string()
                } else {
                    model.to_string()
                },
                api_key: api_key.to_string(),
                base_url: base_url.to_string(),
            })
        } else {
            None
        };
        let fallback = if !fb_provider.is_empty() && !fb_api_key.is_empty() {
            Some(ImageGenCreds {
                provider: fb_provider.to_string(),
                model: if fb_model.is_empty() {
                    providers::image_gen::default_image_model(fb_provider).to_string()
                } else {
                    fb_model.to_string()
                },
                api_key: fb_api_key.to_string(),
                base_url: fb_base_url.to_string(),
            })
        } else {
            None
        };
        Self { primary, fallback }
    }

    /// 主备凭证均未配置时返回 `true`；`image_gen` 工具会据此提前报错。
    pub fn is_empty(&self) -> bool {
        self.primary.is_none() && self.fallback.is_none()
    }
}

/// 单次工具调用的共享运行时上下文，由 AgentLoop 在每次 `dispatch_tool` 前构造。
pub struct ToolContext<'a> {
    /// 当前 Agent 的记忆管理器；`memory_*` 与 `create_agent`（激活时）会修改此字段。
    pub memory: &'a mut MemoryManager,
    /// Agent 根目录（`~/.astro`），用于定位 `agents/{id}/` 等全局路径。
    pub memory_dir: PathBuf,
    /// 当前 Agent 工作区目录，文件类工具的操作根路径。
    pub workspace_dir: PathBuf,
    /// 图片生成主备凭证，由前端 Provider 面板注入。
    pub image_gen_targets: &'a ImageGenTargets,
    /// 已注册的 LLM Provider 列表，供 `image_gen` 查找实现。
    pub providers: &'a ProviderRegistry,
    /// 当前会话 id；`delegate`、`task_plan`、编排落盘时写入关联字段。
    pub session_id: String,
    /// 当前聊天会话的 API Key；`vision`、`tts` 在 Provider 为 OpenAI 时复用。
    pub chat_api_key: String,
    /// 当前聊天会话的 Base URL。
    pub chat_base_url: String,
    /// 当前聊天 Provider id，如 `"openai"`。
    pub chat_provider: String,
    /// 当前聊天模型名称。
    pub chat_model: String,
}

impl<'a> ToolContext<'a> {
    /// 确保 `workspace_dir` 存在；文件/终端/代码执行类工具在操作前调用。
    ///
    /// 返回工作区路径副本，便于后续 `path_safe::resolve_safe` 拼接相对路径。
    pub fn ensure_workspace(&self) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        Ok(self.workspace_dir.clone())
    }
}

/// 便于测试的轻量 Provider 注册表持有类型（`Arc` 共享）。
pub type SharedProviders = Arc<ProviderRegistry>;
