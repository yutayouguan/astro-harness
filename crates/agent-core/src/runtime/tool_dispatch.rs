//! AgentLoop 工具调度：MCP/内置工具路由、HITL 审批、hook 触发与结果变换。

use std::sync::Arc;

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
    /// MCP 工具通过克隆 `Arc<TokioMutex<McpHub>>` 构造动态 handler。
    async fn dispatch_named_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
        workspace_write_grant: bool,
        network_grant: tools::InProcessNetworkGrant,
        step_context: Option<&super::StepContext>,
    ) -> anyhow::Result<types::ToolOutput> {
        let agent_id = self.agent_id.clone();
        self.services
            .tool_registry
            .write()
            .await
            .reload_enabled_from_disk(Some(&agent_id));

        let is_mcp_broker = matches!(name, mcp::MCP_RESOURCES_TOOL | mcp::MCP_PROMPTS_TOOL);

        // MCP 工具：同步 enablement + 刷新注册
        if is_mcp_tool_name(name) || is_mcp_broker {
            let _ = self.mcp_hub.lock().await.sync_enablement_from_disk();
            self.attach_mcp_tools().await;
        }

        let (allowed, registered_handler) = {
            let tool_registry = self.services.tool_registry.read().await;
            (
                tool_registry.is_tool_allowed(name)
                    && step_context.is_none_or(|context| context.advertises_tool(name)),
                tool_registry.dynamic_handler_cloned(name),
            )
        };

        // 在构造 ToolContext 之前，从 Hub 解析 peer（lock → resolve → release）
        // 构建 MCP 动态 handler，持有 Peer（Send + Sync），无需跨 await 持锁。
        let mcp_handler: Option<DynToolHandler> = if is_mcp_tool_name(name) {
            let (peer, native, timeout_secs) = self.mcp_hub.lock().await.resolve_tool_peer(name)?;
            let qname = name.to_string();
            Some(Arc::new(move |_name: &str, args: &serde_json::Value| {
                let peer = peer.clone();
                let qname = qname.clone();
                let native = native.clone();
                let a = args.clone();
                Box::pin(async move {
                    call_tool_with_peer(&peer, &qname, &native, &a, timeout_secs).await
                })
                    as std::pin::Pin<
                        Box<
                            dyn std::future::Future<Output = anyhow::Result<types::ToolOutput>>
                                + Send,
                        >,
                    >
            }))
        } else if name == mcp::MCP_RESOURCES_TOOL {
            let hub = Arc::clone(&self.mcp_hub);
            Some(Arc::new(move |_name: &str, args: &serde_json::Value| {
                let hub = Arc::clone(&hub);
                let args = args.clone();
                Box::pin(async move { mcp::call_resource_broker(&hub, &args).await })
            }))
        } else if name == mcp::MCP_PROMPTS_TOOL {
            let hub = Arc::clone(&self.mcp_hub);
            Some(Arc::new(move |_name: &str, args: &serde_json::Value| {
                let hub = Arc::clone(&hub);
                let args = args.clone();
                Box::pin(async move { mcp::call_prompt_broker(&hub, &args).await })
            }))
        } else {
            None
        };

        let workspace_dir = self.resolve_workspace_dir();
        skills::set_workspace_override(&workspace_dir);
        let session_id = self.session_id.clone();
        let fallback_turn_id = self.current_turn_id().await;
        let turn_id = step_context
            .map(|step_context| step_context.turn.sub_id().to_string())
            .or(fallback_turn_id);
        let memory_dir = self.config.memory_dir.clone();
        let sessions: &dyn ConversationStore = &self.services.sessions;
        let execution = Some(self.execution());
        let hook_bus = Some(self.hook_bus());
        let model_ctx = self.model_context_snapshot().await;
        let (project_root, permission_profile, skill_config_overrides) = {
            let state = self.lock_state();
            (
                state.project_root.clone(),
                state.permission_profile.clone(),
                state.skill_config_overrides.clone(),
            )
        };
        let mut memory = self.services.memory.lock().await;
        let mut ctx = ToolContext {
            memory: &mut memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: step_context
                .and_then(|step_context| step_context.turn.project_root().map(ToOwned::to_owned))
                .or(project_root),
            image_gen_targets: &model_ctx.image_gen_targets,
            session_id,
            turn_id,
            credentials: &model_ctx.credentials,
            chat_targets: &model_ctx.chat_targets,
            execution,
            permission_profile: step_context
                .and_then(|step_context| step_context.turn.permission_profile().map(str::to_string))
                .or(permission_profile),
            skill_config_overrides: &skill_config_overrides,
            hook_bus,
            workspace_write_grant,
            network_grant,
        };
        let dynamic_handler = mcp_handler.or(registered_handler);
        dispatch_tool(|_| allowed, &mut ctx, name, args, dynamic_handler.as_ref()).await
    }

    /// 同步执行工具调用：multi-thread runtime 使用 `block_in_place`；current-thread
    /// runtime 在 scoped worker 中自建 runtime，避免 Tokio 的嵌套阻塞限制。
    ///
    /// 适用于 Tauri 等同步边界；异步上下文优先使用 [`handle_tool_call_async`]。
    pub fn handle_tool_call(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<types::ToolOutput, ToolCallError> {
        self.handle_tool_call_scoped(name, args, false, tools::InProcessNetworkGrant::default())
    }

    /// 执行已审批的单次调用。授权只进入本次 ToolContext，不保存到 Agent 状态。
    pub(crate) fn handle_tool_call_with_once_grants(
        &self,
        name: &str,
        args: &serde_json::Value,
        workspace_write_grant: bool,
        network_grant: tools::InProcessNetworkGrant,
    ) -> Result<types::ToolOutput, ToolCallError> {
        self.handle_tool_call_scoped(name, args, workspace_write_grant, network_grant)
    }

    fn handle_tool_call_scoped(
        &self,
        name: &str,
        args: &serde_json::Value,
        workspace_write_grant: bool,
        network_grant: tools::InProcessNetworkGrant,
    ) -> Result<types::ToolOutput, ToolCallError> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle)
                if matches!(
                    handle.runtime_flavor(),
                    tokio::runtime::RuntimeFlavor::MultiThread
                ) =>
            {
                let fut = self.handle_tool_call_async_scoped(
                    name,
                    args,
                    workspace_write_grant,
                    network_grant,
                );
                tokio::task::block_in_place(|| handle.block_on(fut))
            }
            Ok(_) => std::thread::scope(|scope| {
                scope
                    .spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| ToolCallError::Execution(e.into()))?;
                        rt.block_on(self.handle_tool_call_async_scoped(
                            name,
                            args,
                            workspace_write_grant,
                            network_grant,
                        ))
                    })
                    .join()
                    .unwrap_or_else(|_| {
                        Err(ToolCallError::Execution(anyhow::anyhow!(
                            "tool worker thread panicked"
                        )))
                    })
            }),
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| ToolCallError::Execution(e.into()))?;
                rt.block_on(self.handle_tool_call_async_scoped(
                    name,
                    args,
                    workspace_write_grant,
                    network_grant,
                ))
            }
        }
    }

    /// 异步执行单次工具调用：检查取消 → 递增深度 → hooks → 分发 → hooks。
    ///
    /// 返回 [`ToolCallError`] 区分取消（`Cancelled`）、深度耗尽（`DepthExhausted`）
    /// 和执行异常（`Execution`），避免将取消误记为工具失败。
    pub async fn handle_tool_call_async(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<types::ToolOutput, ToolCallError> {
        self.handle_tool_call_async_scoped(
            name,
            args,
            false,
            tools::InProcessNetworkGrant::default(),
        )
        .await
    }

    async fn handle_tool_call_async_scoped(
        &self,
        name: &str,
        args: &serde_json::Value,
        workspace_write_grant: bool,
        network_grant: tools::InProcessNetworkGrant,
    ) -> Result<types::ToolOutput, ToolCallError> {
        if self.cancel.is_cancelled() {
            return Err(ToolCallError::Cancelled);
        }
        self.increment_tool_round().await?;
        let turn_id = self.current_turn_id().await;
        // 可拦截：PluginHookBus 优先
        let bus_out = self.fire_hook(
            ::hooks::PRE_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id,
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
        let is_skill_soft_alias = {
            let tool_registry = self.services.tool_registry.read().await;
            !is_mcp_tool_name(name)
                && !tool_registry.has_tool(name)
                && tool_registry.is_tool_allowed("skills")
        };
        let (exec_name, exec_args) = if is_skill_soft_alias
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
        let (step_context, interaction_mode) = {
            let state = self.lock_state();
            let step_context = state.current_step_context.clone();
            let interaction_mode = step_context
                .as_ref()
                .map(|step_context| step_context.turn.mode())
                .unwrap_or(state.interaction_mode);
            (step_context, interaction_mode)
        };
        if let Err(msg) = tools::check_tool_call(interaction_mode, exec_name, &exec_args) {
            return Ok(msg.into());
        }
        if step_context
            .as_ref()
            .is_some_and(|step_context| !step_context.advertises_tool(exec_name))
        {
            return Ok(format!(
                "工具 `{exec_name}` 不在生成本次调用的 StepContext 中，已拒绝执行。"
            )
            .into());
        }
        let raw_result = self
            .dispatch_named_tool(
                exec_name,
                &exec_args,
                workspace_write_grant,
                network_grant,
                step_context.as_deref(),
            )
            .await?;
        if exec_name == "skills" {
            self.activate_skill_toolsets_from_args(&exec_args).await;
        }
        // KeyChoice：`confirm` 是关键决策闸口，记一笔供学习闭环。
        if exec_name == "confirm" {
            memory::try_append_decision(
                self.memory_dir(),
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
            self.lock_state().turn.mark_wrote_disk();
        }
        Ok(self
            .finalize_tool_call_result(exec_name, &exec_args, raw_result)
            .await)
    }

    /// `skills` 工具成功加载后：按 frontmatter `astro_tools` additive 放宽 toolset。
    async fn activate_skill_toolsets_from_args(&self, args: &serde_json::Value) {
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
            self.services
                .tool_registry
                .write()
                .await
                .activate_skill_toolsets(&astro_tools);
        }
    }

    /// 统一应用工具结果 hook 与媒体保留逻辑。
    pub(crate) async fn finalize_tool_call_result(
        &self,
        name: &str,
        args_owned: &serde_json::Value,
        raw_result: types::ToolOutput,
    ) -> types::ToolOutput {
        let raw_text = raw_result.text().to_string();
        let turn_id = self.current_turn_id().await;
        let transformed = self.fire_hook(
            ::hooks::TRANSFORM_TOOL_RESULT,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id: turn_id.clone(),
                tool_name: Some(name.into()),
                tool_args: Some(args_owned.clone()),
                tool_result: Some(raw_text.clone()),
                ..Default::default()
            },
        );
        let result = match transformed {
            ::hooks::HookOutcome::ReplaceText(s) => match raw_result {
                types::ToolOutput::Media { assets, .. } => {
                    types::ToolOutput::Media { text: s, assets }
                }
                _ => types::ToolOutput::from(s),
            },
            _ => raw_result,
        };
        let _ = self.fire_hook(
            ::hooks::POST_TOOL_CALL,
            ::hooks::HookPayload {
                session_id: self.session_id.clone(),
                turn_id,
                tool_name: Some(name.into()),
                tool_result: Some(result.text().to_string()),
                detail: {
                    let preview: String = result.text().chars().take(200).collect();
                    format!("{name} → {preview}")
                },
                ..Default::default()
            },
        );
        result
    }
}
