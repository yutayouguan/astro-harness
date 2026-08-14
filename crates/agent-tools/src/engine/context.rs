//! 工具执行上下文：由 AgentLoop 注入的运行时依赖与凭证。
//!
//! 凭证数据类型已下沉到 `types::credentials`，本模块 re-export 并提供
//! `ToolContext` 结构体与 Provider 相关的便捷构造。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use memory::MemoryManager;
use session::ConversationStore;

pub use types::credentials::{ImageGenCreds, ImageGenParts, ImageGenTargets, ModelCredentials};

/// 使用 Provider 默认模型名构建 ImageGenTargets。
pub fn image_gen_targets_from_parts(p: ImageGenParts<'_>) -> ImageGenTargets {
    ImageGenTargets::from_parts(p, |provider| {
        providers::image_gen::default_image_model(provider).to_string()
    })
}

/// 单次工具调用的共享运行时上下文，由 AgentLoop 在每次 `dispatch_tool` 前构造。
pub struct ToolContext<'a> {
    /// 当前 Agent 的记忆管理器；`memory_*` 与 `persona_create`（激活时）会修改此字段。
    pub memory: &'a mut MemoryManager,
    /// 共享会话库（`{memory_dir}/sessions`），供 `search` 使用。
    pub sessions: &'a dyn ConversationStore,
    /// Agent 根目录（`~/.astro`），用于定位 `agents/{id}/` 等全局路径。
    pub memory_dir: PathBuf,
    /// 当前 Agent 工作区目录（记忆空间），与代码仓分离。
    pub workspace_dir: PathBuf,
    /// 可选代码/项目根（git worktree 或 `ASTRO_PROJECT_ROOT`）；有值时 terminal/file_ops 以此为根。
    pub project_root: Option<PathBuf>,
    /// 媒体生成主备凭证，由前端 Provider 面板注入。
    pub image_gen_targets: &'a ImageGenTargets,
    /// 当前会话 id；`delegate`、`todo`、编排落盘时写入关联字段。
    pub session_id: String,
    /// 当前流式 run 的 turn_id（与 agent `run_id` 相同）；未在 run 内为 `None`。
    pub turn_id: Option<String>,
    /// 当前聊天会话的 LLM 凭证（provider / model / api_key / base_url）。
    pub credentials: &'a ModelCredentials,
    /// 含 primary 的聊天 fallback 链，供 `delegate` 下传给子 Agent。
    pub chat_targets: &'a [types::ChatTarget],
    /// 子 Agent 执行调度器（由 AgentLoop 注入；工具层测试可为 None）。
    pub execution: Option<Arc<dyn crate::ExecutionDispatch>>,
    /// 插件钩子总线（由 AgentLoop 注入；无 bus 时对应工具跳过 transform 钩子）。
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

impl<'a> ToolContext<'a> {
    /// 确保记忆工作区存在。
    pub fn ensure_workspace(&self) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        Ok(self.workspace_dir.clone())
    }

    /// 优先 project_root，否则 workspace_dir。
    pub fn project_or_workspace(&self) -> &Path {
        self.project_root
            .as_deref()
            .unwrap_or(self.workspace_dir.as_path())
    }

    /// 确保 [`Self::project_or_workspace`] 目录存在。
    pub fn ensure_project_or_workspace(&self) -> anyhow::Result<PathBuf> {
        let root = self.project_or_workspace().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_parts_writes_primary_music_model_and_leaves_fallback_music_model_empty() {
        let targets = image_gen_targets_from_parts(ImageGenParts {
            provider: "google",
            model: "image-model",
            api_key: "primary-key",
            base_url: "primary-base",
            fb_provider: "openai",
            fb_model: "fallback-image-model",
            fb_api_key: "fallback-key",
            fb_base_url: "fallback-base",
            video_model: "",
            music_model: "  configured-music-model  ",
            tts_model: "",
            fb_video_model: "",
            fb_music_model: "",
            fb_tts_model: "",
            vision_model: "",
            fb_vision_model: "",
        });

        let primary = targets.primary.as_ref().expect("primary credentials");
        assert_eq!(primary.music_model, "configured-music-model");
        let fallback = targets.fallback.as_ref().expect("fallback credentials");
        assert_eq!(fallback.music_model, "");
    }
}
