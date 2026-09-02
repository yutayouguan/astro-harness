//! AgentLoop 工具调度：MCP/内置工具路由、HITL 审批、hook 触发与结果变换。

use std::sync::Arc;

use mcp::{call_tool_with_peer, is_mcp_tool_name};
use session::ConversationStore;
use tokio_util::sync::CancellationToken;
use tools::{dispatch_tool, DynToolHandler, ToolContext};

use super::{AgentLoop, StepContext, ToolInvocation};

/// 工具调用错误：区分取消、深度耗尽与执行异常，避免将取消误记为 ToolFailure。
#[derive(Debug)]
pub enum ToolCallError {
    /// 用户或上层触发了取消。
    Cancelled,
    /// 沙箱进程被拒绝，保留其结构化输出以供重试。
    SandboxDenied(sandbox::SandboxErr),
    /// 工具深度耗尽。
    DepthExhausted(super::turn_budget::MaxDepthError),
    /// 工具执行或分发错误。
    Execution(anyhow::Error),
}

impl std::fmt::Display for ToolCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "prompt cancelled"),
            Self::SandboxDenied(error) => write!(f, "{error}"),
            Self::DepthExhausted(e) => write!(f, "{e}"),
            Self::Execution(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ToolCallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(e) => Some(e.as_ref()),
            Self::SandboxDenied(error) => Some(error),
            Self::DepthExhausted(e) => Some(e),
            Self::Cancelled => None,
        }
    }
}

impl From<anyhow::Error> for ToolCallError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<sandbox::SandboxErr>() {
            Ok(error @ sandbox::SandboxErr::Denied { .. }) => Self::SandboxDenied(error),
            Ok(error) => Self::Execution(error.into()),
            Err(error) => Self::Execution(error),
        }
    }
}

impl From<super::turn_budget::MaxDepthError> for ToolCallError {
    fn from(e: super::turn_budget::MaxDepthError) -> Self {
        Self::DepthExhausted(e)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ToolExecutionGrants {
    pub(crate) workspace_write: bool,
    pub(crate) sandbox_policy: Option<sandbox::SandboxPolicy>,
    pub(crate) managed_network: Option<std::sync::Arc<network_proxy::StartedNetworkProxy>>,
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
        grants: ToolExecutionGrants,
        step_context: Option<&super::StepContext>,
    ) -> anyhow::Result<types::ToolOutput> {
        let wire_name = name;
        let registered_name = step_context
            .map(|step_context| step_context.tool_router.registered_name(wire_name))
            .unwrap_or(wire_name)
            .to_string();
        let name = registered_name.as_str();
        let (agent_id, workspace_dir) = {
            let memory = self.memory();
            (memory.agent_id.clone(), memory.workspace_dir.clone())
        };
        if step_context.is_none() {
            self.services
                .tool_registry
                .write()
                .expect("tool registry lock poisoned")
                .reload_enabled_from_disk(Some(&agent_id));
        }

        let is_mcp_broker = matches!(name, mcp::MCP_RESOURCES_TOOL | mcp::MCP_PROMPTS_TOOL);

        // MCP 工具：同步 enablement + 刷新注册
        if step_context.is_none() && (is_mcp_tool_name(name) || is_mcp_broker) {
            let _ = self.mcp_hub.lock().await.sync_enablement_from_disk();
            self.attach_mcp_tools().await;
        }

        let allowed = step_context.map_or_else(
            || {
                self.services
                    .tool_registry
                    .read()
                    .expect("tool registry lock poisoned")
                    .is_tool_allowed(name)
            },
            |step_context| step_context.tool_router.has_tool(wire_name),
        );

        // 在构造 ToolContext 之前，从 Hub 解析 peer（lock → resolve → release）
        // 构建 MCP 动态 handler，持有 Peer（Send + Sync），无需跨 await 持锁。
        let mcp_handler: Option<DynToolHandler> = if is_mcp_tool_name(name) {
            let (peer, native, timeout_secs, output_token_limit) =
                self.mcp_hub.lock().await.resolve_tool_peer(name)?;
            let qname = name.to_string();
            Some(std::sync::Arc::new(
                move |_name: &str, args: &serde_json::Value| {
                    let peer = peer.clone();
                    let qname = qname.clone();
                    let native = native.clone();
                    let a = args.clone();
                    Box::pin(async move {
                        call_tool_with_peer(
                            &peer,
                            &qname,
                            &native,
                            &a,
                            timeout_secs,
                            output_token_limit,
                        )
                        .await
                    })
                        as std::pin::Pin<
                            Box<
                                dyn std::future::Future<Output = anyhow::Result<types::ToolOutput>>
                                    + Send,
                            >,
                        >
                },
            ))
        } else {
            None
        };
        let dynamic_handler = mcp_handler.or_else(|| {
            step_context.map_or_else(
                || {
                    self.services
                        .tool_registry
                        .read()
                        .expect("tool registry lock poisoned")
                        .dynamic_handler(name)
                },
                |step_context| step_context.tool_router.dynamic_handler(wire_name),
            )
        });

        skills::set_workspace_override(&workspace_dir);
        let session_id = self.session_id.clone();
        let fallback_turn_id = self.current_turn_id().await;
        let turn_id = step_context
            .map(|step_context| step_context.turn.sub_id().to_string())
            .or(fallback_turn_id);
        let memory_dir = self.config.memory_dir.clone();
        let sessions: &dyn ConversationStore = &self.services.sessions;
        let execution = Some(self.execution());
        let hook_runtime = Some(self.hook_runtime());
        let hook_bus = Some(self.hook_bus());
        let (project_root, workspace_roots, permission_profile, model_ctx, skill_config_overrides) = {
            let state = self.lock_state();
            (
                state.project_root.clone(),
                state.workspace_roots.clone(),
                state.permission_profile.clone(),
                state.model_ctx.clone(),
                state.skill_config_overrides.clone(),
            )
        };
        let skill_config_overrides = step_context
            .and_then(|step| step.turn.extension_snapshot())
            .map(|snapshot| snapshot.skill_configs().to_vec())
            .unwrap_or(skill_config_overrides);
        let mut service_tier = step_context
            .and_then(|step| step.turn.provider_settings())
            .and_then(|settings| {
                settings
                    .base_config
                    .additional_params
                    .get("service_tier")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            });
        if self.services.agent_path == subagents::AgentPath::root() {
            self.services
                .agent_control
                .set_root_service_tier(service_tier.clone());
        } else if let Some(root_service_tier) = self.services.agent_control.root_service_tier() {
            service_tier = Some(root_service_tier);
        }
        let mut ctx = ToolContext {
            memory: &self.services.memory,
            sessions,
            memory_dir,
            workspace_dir,
            project_root: step_context
                .and_then(|step_context| step_context.turn.project_root().map(ToOwned::to_owned))
                .or(project_root),
            workspace_roots: step_context
                .map(|step_context| step_context.turn.workspace_roots().to_vec())
                .unwrap_or(workspace_roots),
            image_gen_targets: &model_ctx.image_gen_targets,
            session_id,
            turn_id,
            credentials: &model_ctx.credentials,
            service_tier,
            model_targets: &model_ctx.model_targets,
            execution,
            permission_profile: step_context
                .and_then(|step_context| step_context.turn.permission_profile().map(str::to_string))
                .or(permission_profile),
            skill_config_overrides: &skill_config_overrides,
            hook_bus,
            hook_runtime,
            workspace_write_grant: grants.workspace_write,
            sandbox_policy: grants.sandbox_policy,
            managed_network: grants.managed_network,
            context_window: None,
            context_tokens_used: None,
            tool_registry: Some(&self.services.tool_registry),
        };
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
        self.handle_tool_call_scoped(
            name,
            args,
            ToolExecutionGrants::default(),
            None,
            CancellationToken::new(),
        )
    }

    /// 针对发布该工具的确切采样步骤执行一次调用。
    pub(crate) fn handle_tool_invocation(
        self: &Arc<Self>,
        invocation: ToolInvocation,
    ) -> Result<types::ToolOutput, ToolCallError> {
        self.handle_tool_invocation_with_once_grants(invocation, ToolExecutionGrants::default())
    }

    /// 执行一次绑定到采样步骤的调用，授权范围限于本次尝试。
    pub(crate) fn handle_tool_invocation_with_once_grants(
        self: &Arc<Self>,
        invocation: ToolInvocation,
        grants: ToolExecutionGrants,
    ) -> Result<types::ToolOutput, ToolCallError> {
        debug_assert!(Arc::ptr_eq(self, &invocation.session));
        tracing::trace!(
            call_id = %invocation.call_id,
            tool_name = %invocation.tool_name,
            "dispatch tool invocation"
        );
        self.handle_tool_call_scoped(
            &invocation.tool_name,
            &invocation.payload,
            grants,
            Some(invocation.step_context),
            invocation.cancellation_token,
        )
    }

    fn handle_tool_call_scoped(
        &self,
        name: &str,
        args: &serde_json::Value,
        grants: ToolExecutionGrants,
        step_context: Option<Arc<StepContext>>,
        cancellation_token: CancellationToken,
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
                    grants,
                    step_context,
                    cancellation_token,
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
                            grants,
                            step_context,
                            cancellation_token,
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
                    grants,
                    step_context,
                    cancellation_token,
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
            ToolExecutionGrants::default(),
            None,
            CancellationToken::new(),
        )
        .await
    }

    async fn handle_tool_call_async_scoped(
        &self,
        name: &str,
        args: &serde_json::Value,
        grants: ToolExecutionGrants,
        explicit_step_context: Option<Arc<StepContext>>,
        cancellation_token: CancellationToken,
    ) -> Result<types::ToolOutput, ToolCallError> {
        if self.cancel.is_cancelled() || cancellation_token.is_cancelled() {
            return Err(ToolCallError::Cancelled);
        }
        self.increment_tool_round().await?;
        let turn_id = self.current_turn_id().await;
        let pre_tool_use =
            self.pre_tool_use_request(turn_id, name, format!("direct:{name}"), args.clone());
        let pre_tool_use = self.run_pre_tool_use_hook(pre_tool_use);
        let mut args_owned = args.clone();
        if pre_tool_use.should_block {
            let reason = pre_tool_use
                .block_reason
                .unwrap_or_else(|| "PreToolUse hook blocked tool execution".into());
            return Ok(format!("[blocked by hook] {reason}").into());
        }
        if let Some(updated_input) = pre_tool_use.updated_input {
            args_owned = updated_input;
        }
        if self.cancel.is_cancelled() || cancellation_token.is_cancelled() {
            return Err(ToolCallError::Cancelled);
        }
        let (step_context, interaction_mode) = {
            let state = self.lock_state();
            let step_context = explicit_step_context.or_else(|| state.current_step_context.clone());
            let interaction_mode = step_context
                .as_ref()
                .map(|step_context| step_context.turn.mode())
                .unwrap_or(state.interaction_mode);
            (step_context, interaction_mode)
        };
        // Soft-alias：模型把 Skill 名当工具名时，改写成 skills(skill_id=…)
        let (has_tool, skills_allowed) = step_context.as_ref().map_or_else(
            || {
                let registry = self
                    .services
                    .tool_registry
                    .read()
                    .expect("tool registry lock poisoned");
                (registry.has_tool(name), registry.is_tool_allowed("skills"))
            },
            |step_context| {
                (
                    step_context.tool_router.has_tool(name),
                    step_context.tool_router.has_tool("skills"),
                )
            },
        );
        let skill_is_enabled = step_context
            .as_ref()
            .and_then(|step| step.turn.extension_snapshot())
            .map_or_else(
                || {
                    skills::list_installed()
                        .into_iter()
                        .any(|skill| skill.name == name && skill.enabled)
                },
                |snapshot| {
                    snapshot
                        .skill_index()
                        .iter()
                        .any(|(skill, _)| skill == name)
                },
            );
        let (exec_name, exec_args) =
            if !is_mcp_tool_name(name) && !has_tool && skills_allowed && skill_is_enabled {
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
        if let Err(msg) = tools::check_tool_call(interaction_mode, exec_name, &exec_args) {
            return Ok(msg.into());
        }
        let raw_result = self
            .dispatch_named_tool(exec_name, &exec_args, grants, step_context.as_deref())
            .await?;
        if exec_name == "skills" {
            self.activate_skill_toolsets_from_args(&exec_args, step_context.as_deref());
        }
        // KeyChoice：`confirm` 是关键决策闸口，记一笔供学习闭环。
        if exec_name == "confirm"
            && self.config.thread_memory_mode == types::ThreadMemoryMode::Enabled
        {
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
    fn activate_skill_toolsets_from_args(
        &self,
        args: &serde_json::Value,
        step_context: Option<&StepContext>,
    ) {
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
            None => match step_context
                .and_then(|step| step.turn.extension_snapshot())
                .map_or_else(
                    || skills::load_skill_by_name(skill_id),
                    |snapshot| {
                        skills::load_skill_by_name_with_config(skill_id, snapshot.skill_configs())
                    },
                ) {
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
                .expect("tool registry lock poisoned")
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
                tool_input: Some(args_owned.clone()),
                tool_response: Some(serde_json::Value::String(raw_text.clone())),
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
        let post = self.post_tool_use_request(
            turn_id,
            name,
            format!("direct:{name}"),
            args_owned.clone(),
            serde_json::Value::String(result.text().to_string()),
        );
        let post = self.run_post_tool_use_hook(post);
        let mut model_text = result.text().to_string();
        if post.should_block {
            model_text = format!(
                "Tool result blocked by PostToolUse hook: {}",
                post.feedback_message
                    .as_deref()
                    .unwrap_or("hook requested blocking")
            );
        }
        if !post.additional_contexts.is_empty() {
            model_text.push_str("\n\n[PostToolUse additional context]\n");
            model_text.push_str(&post.additional_contexts.join("\n\n"));
        }
        if !post.should_block {
            if let Some(feedback) = post.feedback_message {
                model_text.push_str("\n\n[PostToolUse feedback]\n");
                model_text.push_str(&feedback);
            }
        }
        let output_token_limit = self
            .services
            .tool_registry
            .read()
            .expect("tool registry lock poisoned")
            .get(name)
            .and_then(|entry| entry.output_token_limit);
        if let Some(tokens) = output_token_limit {
            let max_bytes = tokens
                .saturating_mul(24)
                .div_ceil(5)
                .min(types::MAX_TOOL_RESULT_BYTES);
            model_text = types::truncate_tool_result(&model_text, max_bytes);
        }
        if model_text == result.text() {
            result
        } else {
            match result {
                types::ToolOutput::Media { assets, .. } => types::ToolOutput::Media {
                    text: model_text,
                    assets,
                },
                _ => types::ToolOutput::from(model_text),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn post_tool_use_controls_model_visible_result() {
        let dir = tempfile::tempdir().unwrap();
        let session = AgentLoop::new(super::super::Config::with_defaults(
            dir.path().to_path_buf(),
        ))
        .await
        .unwrap();
        let bus = session.hook_bus();
        bus.register(::hooks::POST_TOOL_USE, |_| {
            ::hooks::HookOutcome::Block("policy rejected output".into())
        });
        bus.register(::hooks::POST_TOOL_USE, |_| {
            ::hooks::HookOutcome::InjectContext("safe replacement context".into())
        });
        bus.register(::hooks::POST_TOOL_USE, |_| {
            ::hooks::HookOutcome::ReplaceText("review this failure".into())
        });

        let output = session
            .finalize_tool_call_result(
                "echo",
                &serde_json::json!({"text": "secret"}),
                types::ToolOutput::from("raw side-effect result"),
            )
            .await;

        assert!(output
            .text()
            .contains("Tool result blocked by PostToolUse hook: policy rejected output"));
        assert!(output.text().contains("safe replacement context"));
        assert!(output.text().contains("review this failure"));
        assert!(!output.text().contains("raw side-effect result"));
    }

    #[tokio::test]
    async fn mcp_output_limit_is_reapplied_after_post_tool_hooks() {
        let dir = tempfile::tempdir().unwrap();
        let session = AgentLoop::new(super::super::Config::with_defaults(
            dir.path().to_path_buf(),
        ))
        .await
        .unwrap();
        session
            .services
            .tool_registry
            .write()
            .unwrap()
            .register(types::ToolEntry {
                name: "mcp__demo__read".into(),
                toolset: mcp::MCP_TOOLSET.into(),
                output_token_limit: Some(1),
                ..types::ToolEntry::lifecycle_defaults()
            });
        session.hook_bus().register(::hooks::POST_TOOL_USE, |_| {
            ::hooks::HookOutcome::InjectContext("hook-expanded-context".repeat(20))
        });

        let output = session
            .finalize_tool_call_result(
                "mcp__demo__read",
                &serde_json::json!({}),
                types::ToolOutput::from("short"),
            )
            .await;

        assert!(output.text().starts_with("short"));
        assert!(output.text().contains("[truncated]"));
    }

    #[test]
    fn tool_execution_grants_default_to_no_managed_network() {
        assert!(ToolExecutionGrants::default().managed_network.is_none());
    }

    #[test]
    fn sandbox_denial_survives_anyhow_dispatch_boundary() {
        let error = anyhow::Error::new(sandbox::SandboxErr::Denied {
            output: Box::new(sandbox::ExecToolCallOutput::new(
                1,
                "partial stdout",
                "Operation not permitted",
            )),
            network_policy_decision: None,
        });

        let error = ToolCallError::from(error);
        assert!(matches!(
            error,
            ToolCallError::SandboxDenied(sandbox::SandboxErr::Denied { output, .. })
                if output.exit_code == 1
        ));

        let setup_error = ToolCallError::from(anyhow::Error::new(
            sandbox::SandboxErr::BackendUnavailable("missing backend".into()),
        ));
        assert!(matches!(setup_error, ToolCallError::Execution(_)));
    }
}
