//! AgentLoop 工具调度：MCP/内置工具路由、HITL 审批、hook 触发与结果变换。

use mcp::{call_tool_with_peer, is_mcp_tool_name};
use session::ConversationStore;
use tools::{dispatch_tool, DynToolHandler, ToolContext};

use super::AgentLoop;

/// 工具调用错误：区分取消、深度耗尽与执行异常，避免将取消误记为 ToolFailure。
#[derive(Debug)]
pub enum ToolCallError {
    /// 用户或上层触发了取消。
    Cancelled,
    /// 工具深度耗尽。
    DepthExhausted(super::turn_budget::MaxDepthError),
    /// 工具执行或分发错误。
    Execution(anyhow::Error),
}

impl std::fmt::Display for ToolCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "prompt cancelled"),
            Self::DepthExhausted(e) => write!(f, "{e}"),
            Self::Execution(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ToolCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(e) => Some(e.as_ref()),
            Self::DepthExhausted(e) => Some(e),
            Self::Cancelled => None,
        }
    }
}

impl From<anyhow::Error> for ToolCallError {
    fn from(e: anyhow::Error) -> Self {
        Self::Execution(e)
    }
}

impl From<super::turn_budget::MaxDepthError> for ToolCallError {
    fn from(e: super::turn_budget::MaxDepthError) -> Self {
        Self::DepthExhausted(e)
    }
}

impl AgentLoop {
    /// 按名称分发工具调用：MCP 走 Hub，内置工具走 [`dispatch_tool`]。
    ///
    /// 调用前刷新 gate 与 MCP 注册；未启用或不存在的工具直接 bail。
    /// MCP 工具通过克隆 `Arc<TokioMutex<McpHub>>` 构造动态 handler，
    /// 避免 `&self.mcp_hub` 与 `&mut self.memory` 的借用冲突。
    async fn dispatch_named_tool(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> anyhow::Result<common::ToolOutput> {
        let agent_id = self.memory.agent_id.clone();
        self.tool_registry.reload_enabled_from_disk(Some(&agent_id));

        // MCP 工具：同步 enablement + 刷新注册
        if is_mcp_tool_name(name) {
            let _ = self.mcp_hub.lock().await.sync_enablement_from_disk();
            self.attach_mcp_tools().await;
        }

        let allowed = self.tool_registry.is_tool_allowed(name);

        // 在构造 ToolContext 之前，从 Hub 解析 peer（lock → resolve → release）
        // 构建 MCP 动态 handler，持有 Peer（Send + Sync），无需跨 await 持锁。
        let mcp_handler: Option<DynToolHandler> = if is_mcp_tool_name(name) {
            let (peer, native, timeout_secs) = self.mcp_hub.lock().await.resolve_tool_peer(name)?;
            let qname = name.to_string();
            Some(Box::new(move |_name: &str, args: &serde_json::Value| {
                let peer = peer.clone();
                let qname = qname.clone();
                let native = native.clone();
                let a = args.clone();
                Box::pin(async move {
                    call_tool_with_peer(&peer, &qname, &native, &a, timeout_secs).await
                })
                    as std::pin::Pin<
                        Box<
                            dyn std::future::Future<Output = anyhow::Result<common::ToolOutput>>
                                + Send,
                        >,
                    >
            }))
        } else {
            None
        };

        let workspace_dir = self.resolve_workspace_dir();
        skills::set_workspace_override(&workspace_dir);
        let session_id = self.session_id.clone();
        let turn_id = self.turn.current_turn_id.clone();
        let memory_dir = self.config.memory_dir.clone();
        let sessions: &dyn ConversationStore = &*self.sessions;
        let execution = Some(self.execution());
        let hook_bus = Some(self.hook_bus());
        let mut ctx = ToolContext {
            memory: &mut self.memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: self.project_root.clone(),
            image_gen_targets: &self.model_ctx.image_gen_targets,
            session_id,
            turn_id,
            credentials: &self.model_ctx.credentials,
            chat_targets: &self.model_ctx.chat_targets,
            execution,
            hook_bus,
        };
        dispatch_tool(|_| allowed, &mut ctx, name, args, mcp_handler.as_ref()).await
    }

    /// 同步执行工具调用：在无 tokio runtime 时自建 current_thread runtime。
    ///
    /// 适用于 Tauri 等同步边界；异步上下文优先使用 [`handle_tool_call_async`]。
    pub fn handle_tool_call(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<common::ToolOutput, ToolCallError> {
        let fut = self.handle_tool_call_async(name, args);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| ToolCallError::Execution(e.into()))?;
                rt.block_on(fut)
            }
        }
    }

    /// 异步执行单次工具调用：检查取消 → 递增深度 → hooks → 分发 → hooks。
    ///
    /// 返回 [`ToolCallError`] 区分取消（`Cancelled`）、深度耗尽（`DepthExhausted`）
    /// 和执行异常（`Execution`），避免将取消误记为工具失败。
    pub async fn handle_tool_call_async(
        &mut self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<common::ToolOutput, ToolCallError> {
        if self.cancel.is_cancelled() {
            return Err(ToolCallError::Cancelled);
        }
        self.increment_tool_round()?;
        // 可拦截：PluginHookBus 优先
        let bus_out = self.fire_hook(
            ::hooks::PRE_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args.clone()),
                detail: format!("{name} {args}"),
                ..Default::default()
            },
        );
        let mut args_owned = args.clone();
        match bus_out {
            ::hooks::HookOutcome::Block(reason) => {
                return Ok(format!("[blocked by hook] {reason}").into());
            }
            ::hooks::HookOutcome::Modify(v) => {
                args_owned = v;
            }
            _ => {}
        }
        if self.cancel.is_cancelled() {
            return Err(ToolCallError::Cancelled);
        }
        // Soft-alias：模型把 Skill 名当工具名时，改写成 skills(skill_id=…)
        let (exec_name, exec_args) = if !is_mcp_tool_name(name)
            && !self.tool_registry.has_tool(name)
            && self.tool_registry.is_tool_allowed("skills")
            && skills::list_installed()
                .into_iter()
                .any(|s| s.name == name && s.enabled)
        {
            (
                "skills",
                serde_json::json!({
                    "action": "load",
                    "skill_id": name,
                    "input": args_owned,
                }),
            )
        } else {
            (name, args_owned)
        };
        if let Err(msg) = tools::check_tool_call(self.interaction_mode, exec_name, &exec_args) {
            return Ok(msg.into());
        }
        let raw_result = self.dispatch_named_tool(exec_name, &exec_args).await?;
        if exec_name == "skills" {
            self.activate_skill_toolsets_from_args(&exec_args);
        }
        // KeyChoice：`confirm` 是关键决策闸口，记一笔供学习闭环。
        if exec_name == "confirm" {
            memory::try_append_decision(
                self.memory.base_dir.as_path(),
                memory::DecisionEntry::new(
                    memory::DecisionKind::KeyChoice,
                    format!(
                        "confirm: {}",
                        exec_args
                            .get("prompt")
                            .or_else(|| exec_args.get("message"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .chars()
                            .take(160)
                            .collect::<String>()
                    ),
                )
                .with_tool("confirm")
                .with_session(self.session_id.clone()),
            );
        }
        if super::tool_writes_disk(exec_name, &exec_args) {
            self.turn.mark_wrote_disk();
        }
        Ok(self
            .finalize_tool_call_result(exec_name, &exec_args, raw_result)
            .await)
    }

    /// `skills` 工具成功加载后：按 frontmatter `astro_tools` additive 放宽 toolset。
    fn activate_skill_toolsets_from_args(&mut self, args: &serde_json::Value) {
        let Some(skill_id) = args
            .get("skill_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        else {
            return;
        };
        // 优先复用 skills 工具刚加载过的结果，避免二次扫描 + 读盘。
        let astro_tools = match skills::recent_astro_tools(skill_id) {
            Some(tools) => tools,
            None => match skills::load_skill_by_name(skill_id) {
                Ok(loaded) => loaded.metadata.astro_tools,
                Err(_) => return,
            },
        };
        if !astro_tools.is_empty() {
            tracing::info!(
                skill = %skill_id,
                toolsets = ?astro_tools,
                "skill activated toolsets (additive)"
            );
            self.tool_registry.activate_skill_toolsets(&astro_tools);
        }
    }

    /// `pub(crate)`：供 `exec::delegate` 的 `subagent_start`/`subagent_stop` 顺序测试复用。
    pub(crate) async fn finalize_tool_call_result(
        &self,
        name: &str,
        args_owned: &serde_json::Value,
        raw_result: common::ToolOutput,
    ) -> common::ToolOutput {
        let raw_text = raw_result.text().to_string();
        let transformed = self.fire_hook(
            ::hooks::TRANSFORM_TOOL_RESULT,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args_owned.clone()),
                tool_result: Some(raw_text.clone()),
                ..Default::default()
            },
        );
        let result = match transformed {
            ::hooks::HookOutcome::ReplaceText(s) => match raw_result {
                common::ToolOutput::Media { assets, .. } => {
                    common::ToolOutput::Media { text: s, assets }
                }
                _ => common::ToolOutput::from(s),
            },
            _ => raw_result,
        };
        let _ = self.fire_hook(
            ::hooks::POST_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: self.turn.current_turn_id.clone(),
                tool_name: Some(name.into()),
                tool_result: Some(result.text().to_string()),
                detail: {
                    let preview: String = result.text().chars().take(200).collect();
                    format!("{name} → {preview}")
                },
                ..Default::default()
            },
        );
        if name == "subagent" || name == "pipeline" {
            self.fire_subagent_stop_from_delegate_result(&raw_text)
                .await;
        }
        result
    }

    async fn fire_subagent_stop_from_delegate_result(&self, result: &str) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(result) else {
            return;
        };
        let tasks = v
            .get("tasks")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_else(|| {
                if v.get("session_id").is_some() {
                    vec![v.clone()]
                } else {
                    Vec::new()
                }
            });
        for t in tasks {
            let child = t
                .get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let summary = t.get("summary").and_then(|s| s.as_str()).unwrap_or("");
            let _ = self.fire_hook(
                ::hooks::SUBAGENT_STOP,
                ::hooks::HookPayload {
                    session_id: child.into(),
                    turn_id: self.turn.current_turn_id.clone(),
                    detail: summary.chars().take(200).collect(),
                    ..Default::default()
                },
            );
        }
    }
}
