//! 将用户保存的智能工作流投影为 Responses API 原生 namespace 工具。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use types::{
    ExecApprovalRequirement, NamespacedToolDef, ToolExposure, ToolName, ToolOutput, ToolSpec,
};
use workflow::engine::{execute_workflow_as_agent_tool, RuntimeProviderConfig, WorkflowRunResult};
use workflow::model::{
    is_reserved_agent_tool_name, validate_agent_tool_input, NodeCategory, NodeType, Workflow,
    WorkflowToolConfirmation, WorkflowToolExposure,
};
use workflow::run_db::WorkflowRunDb;
use workflow::store::WorkflowStore;

use super::context::ToolContext;
use super::executor::{ToolExecutor, ToolExecutorFuture};
use super::registry::ToolRegistry;

pub const WORKFLOW_TOOLSET: &str = "workflow";
pub const WORKFLOW_NAMESPACE: &str = "workflow";
const WORKFLOW_TOOL_WAIT_SECS: u64 = 30;

fn active_runs() -> &'static Mutex<HashMap<String, CancellationToken>> {
    static RUNS: OnceLock<Mutex<HashMap<String, CancellationToken>>> = OnceLock::new();
    RUNS.get_or_init(|| Mutex::new(HashMap::new()))
}

struct CancelOnDrop {
    token: CancellationToken,
    armed: bool,
}

impl CancelOnDrop {
    fn new(token: CancellationToken) -> Self {
        Self { token, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
}

struct ActiveRunRegistration {
    run_id: String,
}

impl Drop for ActiveRunRegistration {
    fn drop(&mut self) {
        if let Ok(mut runs) = active_runs().lock() {
            runs.remove(&self.run_id);
        }
    }
}

fn workflow_requires_approval(workflow: &Workflow) -> bool {
    if workflow.agent_tool.confirmation == WorkflowToolConfirmation::Always {
        return true;
    }
    workflow
        .nodes
        .iter()
        .filter(|node| !node.disabled)
        .any(|node| match node.node_type.category() {
            NodeCategory::Ai | NodeCategory::Media => true,
            _ => match node.node_type {
                NodeType::HttpRequest
                | NodeType::RunLoop
                | NodeType::CustomLoop
                | NodeType::AudioProcessing
                | NodeType::SendNotification
                | NodeType::FileIo
                | NodeType::HumanApproval => true,
                NodeType::Output => node
                    .config
                    .get("export_mode")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|mode| mode != "none"),
                _ => false,
            },
        })
}

fn workflow_may_write_files(workflow: &Workflow) -> bool {
    workflow
        .nodes
        .iter()
        .filter(|node| !node.disabled)
        .any(|node| {
            node.node_type == NodeType::FileIo
                || (node.node_type == NodeType::Output
                    && node
                        .config
                        .get("export_mode")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|mode| matches!(mode, "json" | "folder")))
                || matches!(node.node_type, NodeType::RunLoop | NodeType::CustomLoop)
        })
}

fn workflow_entry(workflow: &Workflow) -> types::ToolEntry {
    let requires_approval = workflow_requires_approval(workflow);
    let always_confirm = workflow.agent_tool.confirmation == WorkflowToolConfirmation::Always
        || workflow.nodes.iter().any(|node| {
            !node.disabled
                && matches!(
                    node.node_type,
                    NodeType::HumanApproval | NodeType::RunLoop | NodeType::CustomLoop
                )
        });
    let exposure = match workflow.agent_tool.exposure {
        WorkflowToolExposure::Disabled => ToolExposure::Hidden,
        WorkflowToolExposure::Deferred => ToolExposure::Deferred,
        WorkflowToolExposure::Direct => ToolExposure::Direct,
    };
    types::ToolEntry {
        name: format!("workflow__{}", workflow.id),
        model_name: Some(workflow.agent_tool_name()),
        toolset: WORKFLOW_TOOLSET.to_string(),
        namespace: WORKFLOW_NAMESPACE.to_string(),
        description: workflow.agent_tool_description(),
        schema: workflow.agent_tool.input_schema.clone(),
        check_fn: None,
        icon: "workflow",
        needs_confirmation: always_confirm,
        approval_requirement: if requires_approval {
            ExecApprovalRequirement::NeedsApproval
        } else {
            ExecApprovalRequirement::Skip
        },
        exposure,
        // 工作流数量无上限；不支持 tool_search 时必须保持不可见。
        allow_eager_fallback: false,
        ..types::ToolEntry::lifecycle_defaults()
    }
}

fn provider_configs_from_context(ctx: &ToolContext<'_>) -> HashMap<String, RuntimeProviderConfig> {
    let mut configs = HashMap::new();
    for target in ctx.model_targets {
        let runtime = RuntimeProviderConfig {
            backend_id: target.backend_id.clone(),
            config: providers::ProviderConfig {
                api_key: target.api_key.clone(),
                base_url: (!target.base_url.trim().is_empty()).then(|| target.base_url.clone()),
                model: target.model.clone(),
                ..providers::ProviderConfig::default()
            },
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            music_model: String::new(),
        };
        configs.insert(target.backend_id.clone(), runtime.clone());
        configs.insert(target.provider_id.clone(), runtime);
    }
    if configs.is_empty() && !ctx.credentials.provider.trim().is_empty() {
        let runtime = RuntimeProviderConfig {
            backend_id: ctx.credentials.provider.clone(),
            config: providers::ProviderConfig {
                api_key: ctx.credentials.api_key.clone(),
                base_url: (!ctx.credentials.base_url.trim().is_empty())
                    .then(|| ctx.credentials.base_url.clone()),
                model: ctx.credentials.model.clone(),
                ..providers::ProviderConfig::default()
            },
            image_model: String::new(),
            video_model: String::new(),
            tts_model: String::new(),
            music_model: String::new(),
        };
        configs.insert(ctx.credentials.provider.clone(), runtime);
    }
    for media in [
        ctx.image_gen_targets.primary.as_ref(),
        ctx.image_gen_targets.fallback.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        let mut merged = false;
        for runtime in configs
            .values_mut()
            .filter(|runtime| runtime.backend_id == media.provider)
        {
            runtime.image_model = media.model.clone();
            runtime.video_model = media.video_model.clone();
            runtime.tts_model = media.tts_model.clone();
            runtime.music_model = media.music_model.clone();
            merged = true;
        }
        if !merged {
            configs.insert(
                media.provider.clone(),
                RuntimeProviderConfig {
                    backend_id: media.provider.clone(),
                    config: providers::ProviderConfig {
                        api_key: media.api_key.clone(),
                        base_url: (!media.base_url.trim().is_empty())
                            .then(|| media.base_url.clone()),
                        model: media.model.clone(),
                        ..providers::ProviderConfig::default()
                    },
                    image_model: String::new(),
                    video_model: String::new(),
                    tts_model: String::new(),
                    music_model: String::new(),
                },
            );
            if let Some(runtime) = configs.get_mut(&media.provider) {
                runtime.image_model = media.model.clone();
                runtime.video_model = media.video_model.clone();
                runtime.tts_model = media.tts_model.clone();
                runtime.music_model = media.music_model.clone();
            }
        }
    }
    configs
}

#[derive(Clone)]
struct WorkflowRuntime {
    entry: types::ToolEntry,
    workflow: Workflow,
}

impl ToolExecutor for WorkflowRuntime {
    fn tool_name(&self) -> ToolName {
        self.entry.tool_name()
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace {
            tools: vec![NamespacedToolDef {
                name: self.workflow.agent_tool_name(),
                description: self.entry.description.clone(),
                schema: self.entry.schema.clone(),
            }],
        }
    }

    fn description(&self) -> &str {
        &self.entry.description
    }

    fn toolset(&self) -> &str {
        WORKFLOW_TOOLSET
    }

    fn approval_requirement(&self) -> ExecApprovalRequirement {
        self.entry.approval_requirement
    }

    fn icon(&self) -> &'static str {
        "workflow"
    }

    fn handle<'a>(
        &'a self,
        ctx: &'a mut ToolContext<'_>,
        args: &'a serde_json::Value,
    ) -> ToolExecutorFuture<'a> {
        let trigger_input = args.clone();
        let workflow = self.workflow.clone();
        if let Err(error) = validate_agent_tool_input(&workflow, &trigger_input) {
            return Box::pin(async move { Err(error) });
        }
        if workflow_may_write_files(&workflow)
            && ctx.active_permission_profile_id() == types::READ_ONLY_PROFILE
            && !ctx.workspace_write_grant
        {
            return Box::pin(async {
                anyhow::bail!(
                    "workflow contains file-writing nodes but no one-shot workspace write grant was issued"
                )
            });
        }
        let provider_configs = provider_configs_from_context(ctx);
        let workflow_db_path = ctx.memory_dir.join("workflows").join("workflow.db");
        Box::pin(async move {
            let run_id = uuid::Uuid::new_v4().to_string();
            let cancellation = CancellationToken::new();
            active_runs()
                .lock()
                .map_err(|_| anyhow::anyhow!("workflow run registry mutex is poisoned"))?
                .insert(run_id.clone(), cancellation.clone());
            // 如果 Agent turn 在等待期间被取消，丢弃工具 future 会联动取消 workflow。
            // 超过前台等待窗口后会解除该联动，让已返回 run_id 的任务继续。
            let mut cancel_on_drop = CancelOnDrop::new(cancellation.clone());

            let task_run_id = run_id.clone();
            let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
            if let Err(error) = std::thread::Builder::new()
                .name(format!("workflow-{task_run_id}"))
                .spawn(move || {
                    let _registration = ActiveRunRegistration {
                        run_id: task_run_id.clone(),
                    };
                    let execution_run_id = task_run_id.clone();
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(anyhow::Error::from)
                        .and_then(|runtime| {
                            runtime.block_on(async move {
                                let run_db = WorkflowRunDb::new(workflow_db_path).await?;
                                let execution = execute_workflow_as_agent_tool(
                                    &workflow,
                                    trigger_input,
                                    &run_db,
                                    provider_configs,
                                    execution_run_id.clone(),
                                );
                                tokio::select! {
                                    result = execution => result,
                                    _ = cancellation.cancelled() => {
                                        let finished_at = chrono::Local::now().to_rfc3339();
                                        run_db.finish_run(
                                            &execution_run_id,
                                            "cancelled",
                                            &finished_at,
                                            Some("cancelled by Agent"),
                                            None,
                                            0,
                                        ).await?;
                                        Ok(WorkflowRunResult {
                                            run_id: execution_run_id.clone(),
                                            status: "cancelled".into(),
                                            output: None,
                                            error: Some("cancelled by Agent".into()),
                                            steps_executed: 0,
                                        })
                                    }
                                }
                            })
                        });
                    let result = result.unwrap_or_else(|error| WorkflowRunResult {
                        run_id: task_run_id.clone(),
                        status: "failure".into(),
                        output: None,
                        error: Some(error.to_string()),
                        steps_executed: 0,
                    });
                    let _ = result_tx.send(result);
                })
            {
                if let Ok(mut runs) = active_runs().lock() {
                    runs.remove(&run_id);
                }
                return Err(anyhow::anyhow!("start workflow worker failed: {error}"));
            }

            let value = match tokio::time::timeout(
                Duration::from_secs(WORKFLOW_TOOL_WAIT_SECS),
                &mut result_rx,
            )
            .await
            {
                Ok(received) => serde_json::to_value(
                    received
                        .map_err(|_| anyhow::anyhow!("workflow worker stopped unexpectedly"))?,
                )?,
                Err(_) => serde_json::json!({
                    "run_id": run_id,
                    "status": "running",
                    "output": null,
                    "error": null,
                    "steps_executed": 0
                }),
            };
            cancel_on_drop.disarm();
            Ok(ToolOutput::from(serde_json::to_string(&value)?))
        })
    }
}

#[derive(Clone, Copy)]
enum RunControlAction {
    Get,
    Cancel,
}

struct RunControlRuntime {
    entry: types::ToolEntry,
    action: RunControlAction,
}

impl ToolExecutor for RunControlRuntime {
    fn tool_name(&self) -> ToolName {
        self.entry.tool_name()
    }

    fn spec(&self) -> ToolSpec {
        ToolSpec::Namespace {
            tools: vec![NamespacedToolDef {
                name: self.entry.tool_name().name().to_string(),
                description: self.entry.description.clone(),
                schema: self.entry.schema.clone(),
            }],
        }
    }

    fn description(&self) -> &str {
        &self.entry.description
    }

    fn toolset(&self) -> &str {
        WORKFLOW_TOOLSET
    }

    fn handle<'a>(
        &'a self,
        ctx: &'a mut ToolContext<'_>,
        args: &'a serde_json::Value,
    ) -> ToolExecutorFuture<'a> {
        let action = self.action;
        let run_id = args
            .get("run_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let workflow_db_path = ctx.memory_dir.join("workflows").join("workflow.db");
        Box::pin(async move {
            anyhow::ensure!(!run_id.is_empty(), "run_id 不能为空");
            let value = match action {
                RunControlAction::Get => {
                    let db = WorkflowRunDb::new(workflow_db_path).await?;
                    match db.get_run(&run_id).await? {
                        Some(row) => serde_json::json!({
                            "run_id": row.id,
                            "status": row.status,
                            "output": row.output.and_then(|output| serde_json::from_str::<serde_json::Value>(&output).ok()),
                            "error": row.error,
                            "steps_executed": row.node_count
                        }),
                        None => serde_json::json!({
                            "run_id": run_id,
                            "status": "not_found",
                            "output": null,
                            "error": "workflow run not found",
                            "steps_executed": 0
                        }),
                    }
                }
                RunControlAction::Cancel => {
                    let cancelled = active_runs()
                        .lock()
                        .map_err(|_| anyhow::anyhow!("workflow run registry mutex is poisoned"))?
                        .get(&run_id)
                        .map(|token| {
                            token.cancel();
                            true
                        })
                        .unwrap_or(false);
                    serde_json::json!({"run_id": run_id, "cancelled": cancelled})
                }
            };
            Ok(ToolOutput::from(serde_json::to_string(&value)?))
        })
    }
}

fn register_run_control(registry: &mut ToolRegistry, name: &str, action: RunControlAction) {
    let requires_approval = matches!(action, RunControlAction::Cancel);
    let entry = types::ToolEntry {
        name: format!("workflow__{name}"),
        model_name: Some(name.to_string()),
        toolset: WORKFLOW_TOOLSET.to_string(),
        namespace: WORKFLOW_NAMESPACE.to_string(),
        description: match action {
            RunControlAction::Get => "查询已启动的智能工作流运行状态。",
            RunControlAction::Cancel => "取消当前进程内仍在运行的智能工作流。",
        }
        .to_string(),
        schema: serde_json::json!({
            "type": "object",
            "properties": {"run_id": {"type": "string", "description": "Workflow run id"}},
            "required": ["run_id"],
            "additionalProperties": false
        }),
        check_fn: None,
        icon: "workflow",
        needs_confirmation: requires_approval,
        approval_requirement: if requires_approval {
            ExecApprovalRequirement::NeedsApproval
        } else {
            ExecApprovalRequirement::Skip
        },
        ..types::ToolEntry::lifecycle_defaults()
    };
    registry.register_runtime(entry.clone(), Arc::new(RunControlRuntime { entry, action }));
}

/// 用当前磁盘快照原子替换 workflow toolset。
///
/// `ToolRouter` 会再次 clone registry，因此模型看到的 schema 和实际执行的
/// workflow 定义属于同一 Step 快照。
pub fn register_workflow_tools(
    registry: &mut ToolRegistry,
    memory_dir: &Path,
) -> anyhow::Result<usize> {
    registry.unregister_toolset(WORKFLOW_TOOLSET);
    if !registry.is_toolset_enabled(WORKFLOW_TOOLSET) {
        return Ok(0);
    }
    let workflows = WorkflowStore::open(memory_dir.join("workflows"))?.list()?;
    let callable = workflows
        .into_iter()
        .filter(|workflow| {
            workflow.enabled && workflow.agent_tool.exposure != WorkflowToolExposure::Disabled
        })
        .collect::<Vec<_>>();
    let mut names = HashSet::new();
    for workflow in &callable {
        WorkflowStore::validate_workflow(workflow)?;
        let name = workflow.agent_tool_name();
        anyhow::ensure!(
            !is_reserved_agent_tool_name(&name),
            "workflow Agent 工具名 `{name}` 为保留名"
        );
        anyhow::ensure!(
            names.insert(name.clone()),
            "workflow Agent 工具名 `{name}` 重复"
        );
    }
    // 管理工具始终存在：工作流在后台运行期间可能被关闭或删除，
    // 不能因此丢失查询/取消能力。
    register_run_control(registry, "get_run", RunControlAction::Get);
    register_run_control(registry, "cancel_run", RunControlAction::Cancel);
    let count = callable.len();
    for workflow in callable {
        let entry = workflow_entry(&workflow);
        registry.register_runtime(entry.clone(), Arc::new(WorkflowRuntime { entry, workflow }));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;
    use workflow::model::{NewWorkflow, WorkflowAgentTool};

    #[test]
    fn workflow_entry_uses_native_namespace_and_never_eager_falls_back() {
        let mut workflow = Workflow::new(NewWorkflow {
            name: "Weekly Report".into(),
            description: "Build the report".into(),
        });
        workflow.enabled = true;
        workflow.agent_tool = WorkflowAgentTool::deferred();
        workflow.agent_tool.name = "generate_weekly_report".into();
        let entry = workflow_entry(&workflow);
        assert_eq!(
            entry.tool_name(),
            ToolName::namespaced(WORKFLOW_NAMESPACE, "generate_weekly_report")
        );
        assert_eq!(entry.exposure, ToolExposure::Deferred);
        assert!(!entry.allow_eager_fallback);
    }

    #[test]
    fn workflow_side_effects_require_approval() {
        let mut workflow = Workflow::new(NewWorkflow {
            name: "HTTP".into(),
            description: String::new(),
        });
        workflow.nodes.push(workflow::model::WorkflowNode {
            id: "http".into(),
            node_type: NodeType::HttpRequest,
            label: "HTTP".into(),
            position: workflow::model::Position { x: 0.0, y: 0.0 },
            config: serde_json::json!({}),
            disabled: false,
        });
        assert!(workflow_requires_approval(&workflow));
        assert_eq!(
            workflow_entry(&workflow).approval_requirement,
            ExecApprovalRequirement::NeedsApproval
        );
        assert!(!workflow_entry(&workflow).needs_confirmation);

        workflow.agent_tool.confirmation = WorkflowToolConfirmation::Always;
        assert!(workflow_entry(&workflow).needs_confirmation);

        let human = workflow::model::WorkflowNode {
            id: "human".into(),
            node_type: NodeType::HumanApproval,
            label: "Human".into(),
            position: workflow::model::Position { x: 0.0, y: 0.0 },
            config: serde_json::json!({}),
            disabled: false,
        };
        workflow.agent_tool.confirmation = WorkflowToolConfirmation::Auto;
        workflow.nodes = vec![human];
        assert!(workflow_entry(&workflow).needs_confirmation);
    }

    #[test]
    fn dropping_an_active_wait_cancels_the_workflow() {
        let token = CancellationToken::new();
        {
            let _guard = CancelOnDrop::new(token.clone());
        }
        assert!(token.is_cancelled());

        let token = CancellationToken::new();
        {
            let mut guard = CancelOnDrop::new(token.clone());
            guard.disarm();
        }
        assert!(!token.is_cancelled());
    }

    #[test]
    fn registers_only_enabled_non_disabled_workflows() {
        let root = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(root.path().join("workflows")).unwrap();
        let mut callable = store
            .create(NewWorkflow {
                name: "Callable".into(),
                description: String::new(),
            })
            .unwrap();
        callable.enabled = true;
        callable.agent_tool.name = "callable".into();
        store.save_workflow(callable).unwrap();
        let mut disabled = store
            .create(NewWorkflow {
                name: "Disabled".into(),
                description: String::new(),
            })
            .unwrap();
        disabled.enabled = true;
        disabled.agent_tool.exposure = WorkflowToolExposure::Disabled;
        store.save_workflow(disabled).unwrap();

        let mut registry = ToolRegistry::new();
        assert_eq!(
            register_workflow_tools(&mut registry, root.path()).unwrap(),
            1
        );
        assert!(registry
            .all_tools()
            .iter()
            .any(|entry| entry.tool_name() == ToolName::namespaced("workflow", "callable")));
        assert!(!registry
            .all_tools()
            .iter()
            .any(|entry| entry.description.contains("Disabled")));

        let initial = registry.schemas_for_api();
        let workflow_namespace = initial
            .iter()
            .find(|spec| spec.get("name").and_then(serde_json::Value::as_str) == Some("workflow"));
        assert_eq!(
            workflow_namespace
                .and_then(|spec| spec.get("tools"))
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(2),
            "only get_run and cancel_run are direct before discovery"
        );

        let discovered = HashSet::from([ToolName::namespaced("workflow", "callable")]);
        let (_, callable_specs) = registry.schemas_for_step(&discovered);
        assert!(callable_specs.iter().any(|spec| {
            spec.get("name").and_then(serde_json::Value::as_str) == Some("workflow")
                && spec
                    .get("tools")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|children| {
                        children.iter().any(|child| {
                            child.get("name").and_then(serde_json::Value::as_str)
                                == Some("callable")
                        })
                    })
        }));

        let catalog = crate::catalog_for_ui(&registry);
        let workflow_catalog = catalog
            .iter()
            .find(|item| item.id == WORKFLOW_TOOLSET)
            .expect("workflow catalog");
        assert!(workflow_catalog.functions.iter().any(|function| {
            function.name == "workflow.callable" && function.exposure == "deferred"
        }));
    }

    #[tokio::test]
    async fn registered_workflow_executes_with_structured_result() {
        let root = tempfile::tempdir().expect("tempdir");
        let store = WorkflowStore::open(root.path().join("workflows")).expect("workflow store");
        let mut workflow = store
            .create(NewWorkflow {
                name: "Callable".into(),
                description: String::new(),
            })
            .expect("create workflow");
        workflow.enabled = true;
        workflow.agent_tool.name = "callable".into();
        workflow.agent_tool.input_schema = serde_json::json!({
            "type": "object",
            "properties": {"question": {"type": "string"}},
            "required": ["question"],
            "additionalProperties": false
        });
        store.save_workflow(workflow).expect("save workflow");

        let mut registry = ToolRegistry::new();
        register_workflow_tools(&mut registry, root.path()).expect("register workflow tools");
        let registered_name = registry
            .all_tools()
            .into_iter()
            .find(|entry| entry.tool_name() == ToolName::namespaced("workflow", "callable"))
            .expect("registered workflow")
            .name
            .clone();

        let memory = RwLock::new(
            memory::MemoryManager::new(root.path().to_path_buf()).expect("memory manager"),
        );
        let sessions = session::SessionStore::open_sessions_dir(&root.path().join("data"))
            .await
            .expect("session store");
        let targets = crate::ImageGenTargets::default();
        let credentials = crate::ModelCredentials::default();
        let mut context = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: root.path().to_path_buf(),
            workspace_dir: root.path().join("workspace"),
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "workflow-tool-test".into(),
            turn_id: None,
            credentials: &credentials,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };
        let output = registry
            .dispatch(
                &mut context,
                &registered_name,
                &serde_json::json!({"question": "hello"}),
            )
            .await
            .expect("execute workflow tool");
        let result: serde_json::Value =
            serde_json::from_str(output.text()).expect("structured result");
        assert_eq!(result["status"], "success");
        assert!(result["run_id"].as_str().is_some_and(|id| !id.is_empty()));

        let error = registry
            .dispatch(&mut context, &registered_name, &serde_json::json!({}))
            .await
            .expect_err("invalid workflow input must fail closed");
        assert!(error
            .to_string()
            .contains("workflow 输入不符合 JSON Schema"));
    }

    #[test]
    fn disabled_workflow_toolset_skips_disk_catalog() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(root.path().join("workflows")).expect("workflow directory");
        std::fs::write(root.path().join("workflows/workflows.json"), "{invalid")
            .expect("invalid workflow fixture");
        let mut registry = ToolRegistry::new();
        registry.set_enabled_map(HashMap::from([(WORKFLOW_TOOLSET.to_string(), false)]));

        assert_eq!(
            register_workflow_tools(&mut registry, root.path()).expect("disabled toolset"),
            0
        );
        assert!(registry
            .all_tools()
            .iter()
            .all(|entry| entry.toolset != WORKFLOW_TOOLSET));
    }
}
