//! 工具执行上下文：由 AgentLoop 注入的运行时依赖与凭证。
//!
//! 各内置工具的 `dispatch` 函数通过 [`ToolContext`] 访问工作区路径、
//! 记忆管理器、Provider 注册表及聊天/生图凭证，避免在工具层重复读取环境变量。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use memory::MemoryManager;
use providers::registry::ProviderRegistry;
use session::SessionStore;

/// 单个媒体生成 Provider 的调用凭证。
///
/// 由 Tauri 前端从「模型提供商」面板注入，经 [`ImageGenTargets`] 传给
/// `image_gen` / `video_gen` / `tts` / `vision` 工具。
#[derive(Debug, Clone, Default)]
pub struct ImageGenCreds {
    /// Provider id，如 `"google"`、`"openai"`。
    pub provider: String,
    /// 生图模型；为空时由 `providers::image_gen::default_image_model` 补全。
    pub model: String,
    /// API Key。
    pub api_key: String,
    /// 自定义 Base URL；为空时使用 Provider 默认值。
    pub base_url: String,
    /// 生视频模型（主要为 Google）；空则用 `default_video_model`。
    pub video_model: String,
    /// 音乐生成模型（Google Lyria）；空则使用 clip 默认。
    pub music_model: String,
    /// 生音频 / TTS 模型；空则用供应商默认。
    pub tts_model: String,
    /// 视觉（图片理解）模型；空则用 `default_vision_model`。
    pub vision_model: String,
}

/// 主备媒体凭证对：主 Provider 失败时自动尝试备用（图 / 音）。
#[derive(Debug, Clone, Default)]
pub struct ImageGenTargets {
    /// 优先使用的媒体生成凭证。
    pub primary: Option<ImageGenCreds>,
    /// 主 Provider 失败时的备用凭证。
    pub fallback: Option<ImageGenCreds>,
}

/// `ImageGenTargets::from_parts` 的扁平字符串入参（Tauri / gRPC 注入字段）。
#[derive(Debug, Clone, Copy)]
pub struct ImageGenParts<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub fb_provider: &'a str,
    pub fb_model: &'a str,
    pub fb_api_key: &'a str,
    pub fb_base_url: &'a str,
    pub video_model: &'a str,
    pub music_model: &'a str,
    pub tts_model: &'a str,
    pub fb_tts_model: &'a str,
    pub vision_model: &'a str,
    pub fb_vision_model: &'a str,
}

impl ImageGenTargets {
    /// 从 Tauri / gRPC 注入的字符串字段构建主备凭证。
    ///
    /// Provider 或 API Key 为空时，对应槽位为 `None`；
    /// 图片 model 为空则使用 Provider 默认图片模型。
    pub fn from_parts(p: ImageGenParts<'_>) -> Self {
        let primary = if !p.provider.is_empty() && !p.api_key.is_empty() {
            Some(ImageGenCreds {
                provider: p.provider.to_string(),
                model: if p.model.is_empty() {
                    providers::image_gen::default_image_model(p.provider).to_string()
                } else {
                    p.model.to_string()
                },
                api_key: p.api_key.to_string(),
                base_url: p.base_url.to_string(),
                video_model: p.video_model.trim().to_string(),
                music_model: p.music_model.trim().to_string(),
                tts_model: p.tts_model.trim().to_string(),
                vision_model: p.vision_model.trim().to_string(),
            })
        } else {
            None
        };
        let fallback = if !p.fb_provider.is_empty() && !p.fb_api_key.is_empty() {
            Some(ImageGenCreds {
                provider: p.fb_provider.to_string(),
                model: if p.fb_model.is_empty() {
                    providers::image_gen::default_image_model(p.fb_provider).to_string()
                } else {
                    p.fb_model.to_string()
                },
                api_key: p.fb_api_key.to_string(),
                base_url: p.fb_base_url.to_string(),
                video_model: String::new(),
                music_model: String::new(),
                tts_model: p.fb_tts_model.trim().to_string(),
                vision_model: p.fb_vision_model.trim().to_string(),
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

    /// 按 provider id 查找主或备凭证。
    pub fn find_provider(&self, provider: &str) -> Option<&ImageGenCreds> {
        self.primary
            .as_ref()
            .filter(|c| c.provider == provider)
            .or_else(|| self.fallback.as_ref().filter(|c| c.provider == provider))
    }

    /// Google 媒体生成凭证（图 / 视频 / TTS 共用 key）。
    pub fn google(&self) -> Option<&ImageGenCreds> {
        self.find_provider("google")
    }

    /// OpenAI 备用凭证。
    pub fn openai(&self) -> Option<&ImageGenCreds> {
        self.find_provider("openai")
    }
}

/// 单次工具调用的共享运行时上下文，由 AgentLoop 在每次 `dispatch_tool` 前构造。
pub struct ToolContext<'a> {
    /// 当前 Agent 的记忆管理器；`memory_*` 与 `persona_create`（激活时）会修改此字段。
    pub memory: &'a mut MemoryManager,
    /// 共享会话库（`{memory_dir}/sessions`），供 `search` 使用。
    pub sessions: &'a SessionStore,
    /// Agent 根目录（`~/.astro`），用于定位 `agents/{id}/` 等全局路径。
    pub memory_dir: PathBuf,
    /// 当前 Agent 工作区目录（记忆空间），与代码仓分离。
    pub workspace_dir: PathBuf,
    /// 可选代码/项目根（git worktree 或 `ASTRO_PROJECT_ROOT`）；有值时 terminal/file_ops 以此为根。
    pub project_root: Option<PathBuf>,
    /// 媒体生成主备凭证，由前端 Provider 面板注入。
    pub image_gen_targets: &'a ImageGenTargets,
    /// 已注册的 LLM Provider 列表，供 `image_gen` 查找实现。
    pub providers: &'a ProviderRegistry,
    /// 当前会话 id；`delegate`、`todo`、编排落盘时写入关联字段。
    pub session_id: String,
    /// 当前流式 run 的 turn_id（与 agent `run_id` 相同）；未在 run 内为 `None`。
    pub turn_id: Option<String>,
    /// 当前聊天会话的 API Key；`vision`、`tts` 在 Provider 为 OpenAI 时复用。
    pub chat_api_key: String,
    /// 当前聊天会话的 Base URL。
    pub chat_base_url: String,
    /// 当前聊天 Provider id，如 `"openai"`。
    pub chat_provider: String,
    /// 当前聊天模型名称。
    pub chat_model: String,
    /// 含 primary 的聊天 fallback 链，供 `delegate` 下传给子 Agent。
    pub chat_targets: Vec<common::ChatTarget>,
    /// 同步委派执行器（由 AgentLoop 注入；工具层测试可为 None）。
    pub delegate_runner: Option<delegate::DelegateRunner>,
    /// 异步委派 spawner（由 AgentLoop 注入；工具层测试可为 None）。
    pub async_spawner: Option<delegate::DelegateAsyncSpawner>,
    /// 编排 spawner（由 AgentLoop 注入；工具层测试可为 None）。
    pub orchestration_spawner: Option<orchestration::OrchestrationSpawner>,
    /// 插件钩子总线（由 AgentLoop 注入；无 bus 时对应工具跳过 transform 钩子）。
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

impl<'a> ToolContext<'a> {
    /// 确保记忆工作区存在。
    pub fn ensure_workspace(&self) -> anyhow::Result<PathBuf> {
        std::fs::create_dir_all(&self.workspace_dir)?;
        Ok(self.workspace_dir.clone())
    }

    /// 终端 / 文件操作的沙箱根：优先 `project_root`，否则记忆 `workspace_dir`。
    pub fn project_or_workspace(&self) -> &Path {
        self.project_root.as_ref().unwrap_or(&self.workspace_dir)
    }

    /// 确保 [`Self::project_or_workspace`] 目录存在。
    pub fn ensure_project_or_workspace(&self) -> anyhow::Result<PathBuf> {
        let root = self.project_or_workspace().to_path_buf();
        std::fs::create_dir_all(&root)?;
        Ok(root)
    }
}

/// 便于测试的轻量 Provider 注册表持有类型（`Arc` 共享）。
pub type SharedProviders = Arc<ProviderRegistry>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_parts_writes_primary_music_model_and_leaves_fallback_music_model_empty() {
        let targets = ImageGenTargets::from_parts(ImageGenParts {
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
