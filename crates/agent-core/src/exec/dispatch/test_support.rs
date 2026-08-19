use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::stream;
use providers::types::stream::StreamChunk;
use providers::CompletionStream;
use subagents::{
    AgentControl, AgentGraphStore, AgentPath, AgentStatusKind, AgentStatusV2, AgentThreadV2,
    AgentTreeSnapshotV2, InterruptAgentV2Request, InterruptAgentV2Result, Limits,
    MessageAgentV2Request, MessageAgentV2Result, SpawnAgentV2Request,
};
use tools::{
    AgentThreadDispatch, FollowupAgentDispatchRequest, ParentRuntimeMaterial,
    SpawnAgentDispatchRequest,
};

use super::{
    DefaultAgentThreadDispatch, DefaultDesktopAgentThreadControl, DesktopAgentThreadControl,
    RuntimeRequestRegistry,
};
use crate::exec::agent_runtime::AgentRuntimeManager;
use crate::streaming::ChatOverride;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptedTurn {
    Complete(String),
    Pending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedProviderCall {
    pub messages: Vec<(String, String)>,
    pub tool_names: Vec<String>,
    pub model: String,
    pub additional_params: serde_json::Value,
}

pub struct LifecycleTestApp {
    memory_dir: PathBuf,
    root_thread_id: String,
    control: Arc<AgentControl>,
    runtime_manager: Arc<AgentRuntimeManager>,
    runtime_requests: Arc<RuntimeRequestRegistry>,
    chat_override: ChatOverride,
    provider_calls: Arc<Mutex<Vec<CapturedProviderCall>>>,
    hook_bus: Arc<hooks::PluginHookBus>,
    hook_events: Arc<Mutex<Vec<String>>>,
}

fn scripted_chat(
    script: Vec<ScriptedTurn>,
) -> (ChatOverride, Arc<Mutex<Vec<CapturedProviderCall>>>) {
    let script = Arc::new(Mutex::new(VecDeque::from(script)));
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&calls);
    let chat: ChatOverride = Arc::new(
        move |messages: Vec<providers::Message>,
              tools: Vec<serde_json::Value>,
              config: providers::ProviderConfig| {
            observed.lock().unwrap().push(CapturedProviderCall {
                messages: messages
                    .iter()
                    .map(|message| {
                        (
                            message.role().as_str().to_string(),
                            message.text_content().to_string(),
                        )
                    })
                    .collect(),
                tool_names: tools
                    .iter()
                    .filter_map(|tool| {
                        tool.get("function")
                            .and_then(|function| function.get("name"))
                            .or_else(|| tool.get("name"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string)
                    })
                    .collect(),
                model: config.model.clone(),
                additional_params: config.additional_params.clone(),
            });
            let next = script
                .lock()
                .expect("script mutex")
                .pop_front()
                .expect("unexpected provider turn");
            Box::pin(async move {
                match next {
                    ScriptedTurn::Complete(reply) => Ok(Box::pin(stream::iter(vec![
                        Ok(StreamChunk::Text(reply)),
                        Ok(StreamChunk::Done {
                            finish_reason: "stop".into(),
                        }),
                    ]))
                        as CompletionStream),
                    ScriptedTurn::Pending => Ok(Box::pin(stream::pending::<
                        anyhow::Result<StreamChunk>,
                    >()) as CompletionStream),
                }
            })
        },
    );
    (chat, calls)
}

fn lifecycle_hooks() -> (Arc<hooks::PluginHookBus>, Arc<Mutex<Vec<String>>>) {
    let hook_bus = Arc::new(hooks::PluginHookBus::new());
    let hook_events = Arc::new(Mutex::new(Vec::new()));
    for (name, label) in [
        (hooks::SUBAGENT_START, "start"),
        (hooks::SUBAGENT_STOP, "stop"),
    ] {
        let observed = Arc::clone(&hook_events);
        hook_bus.register(name, move |payload| {
            let path = payload
                .detail
                .split_whitespace()
                .find_map(|part| part.strip_prefix("path="))
                .unwrap_or("<missing>");
            observed.lock().unwrap().push(format!("{label}:{path}"));
            hooks::HookOutcome::Continue
        });
    }
    (hook_bus, hook_events)
}

impl LifecycleTestApp {
    pub fn new(
        memory_dir: PathBuf,
        root_thread_id: impl Into<String>,
        script: Vec<ScriptedTurn>,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&memory_dir)?;
        let root_thread_id = root_thread_id.into();
        let sessions = session::SessionStore::open_sessions_dir(&memory_dir.join("sessions"))?;
        sessions.ensure_session(&root_thread_id, "acceptance-root")?;
        let store = AgentGraphStore::open(memory_dir.join("subagents-v2.db"))?;
        let control = AgentControl::open(
            root_thread_id.clone(),
            store,
            Limits {
                max_threads: 16,
                max_depth: 8,
                max_running: 4,
            },
        )?;
        let (chat_override, provider_calls) = scripted_chat(script);
        let (hook_bus, hook_events) = lifecycle_hooks();
        Ok(Self {
            memory_dir,
            root_thread_id,
            control,
            runtime_manager: Arc::new(AgentRuntimeManager::default()),
            runtime_requests: Arc::new(RuntimeRequestRegistry::default()),
            chat_override,
            provider_calls,
            hook_bus,
            hook_events,
        })
    }

    fn dispatch_at(&self, path: &str) -> anyhow::Result<DefaultAgentThreadDispatch> {
        let path = AgentPath::parse(path).map_err(anyhow::Error::msg)?;
        let thread = self.control.resolve_desktop_target(path.as_str())?;
        Ok(DefaultAgentThreadDispatch {
            control: Arc::clone(&self.control),
            current_path: path,
            current_thread_id: thread.thread_id,
            runtime_manager: Arc::clone(&self.runtime_manager),
            runtime_requests: Arc::clone(&self.runtime_requests),
            chat_override: Some(Arc::clone(&self.chat_override)),
            #[cfg(test)]
            before_followup_atomic_hook: None,
        })
    }

    fn desktop(&self) -> DefaultDesktopAgentThreadControl {
        DefaultDesktopAgentThreadControl::for_acceptance(
            self.memory_dir.clone(),
            Arc::clone(&self.control),
            Arc::clone(&self.runtime_manager),
            Arc::clone(&self.runtime_requests),
            Arc::clone(&self.chat_override),
        )
    }

    pub async fn spawn(
        &self,
        parent: &str,
        task_name: &str,
        message: &str,
    ) -> anyhow::Result<AgentThreadV2> {
        let request = SpawnAgentDispatchRequest {
            request: SpawnAgentV2Request {
                task_name: task_name.into(),
                message: message.into(),
                agent_type: None,
                model: None,
                reasoning_effort: None,
                fork_turns: Some("none".into()),
            },
            runtime: self.parent_runtime_material(),
        };
        Ok(
            AgentThreadDispatch::spawn_agent(&self.dispatch_at(parent)?, request)
                .await?
                .thread,
        )
    }

    pub async fn send_message(
        &self,
        target: &str,
        message: &str,
    ) -> anyhow::Result<MessageAgentV2Result> {
        AgentThreadDispatch::send_message(
            &self.dispatch_at("/root")?,
            MessageAgentV2Request {
                target: target.into(),
                message: message.into(),
            },
        )
        .await
    }

    pub async fn followup(
        &self,
        target: &str,
        message: &str,
    ) -> anyhow::Result<MessageAgentV2Result> {
        AgentThreadDispatch::followup_task_with_runtime(
            &self.dispatch_at("/root")?,
            FollowupAgentDispatchRequest {
                request: MessageAgentV2Request {
                    target: target.into(),
                    message: message.into(),
                },
                runtime: Some(self.parent_runtime_material()),
            },
        )
        .await
    }

    fn parent_runtime_material(&self) -> ParentRuntimeMaterial {
        ParentRuntimeMaterial {
            memory_dir: self.memory_dir.clone(),
            parent_agent_id: home::DEFAULT_AGENT_ID.into(),
            parent_model: Some("openai:test".into()),
            parent_sandbox_mode: "workspace-write".into(),
            inherited_skill_config: Vec::new(),
            chat_targets: vec![types::ChatTarget {
                provider_id: "test".into(),
                backend_id: "openai".into(),
                model: "test".into(),
                api_key: "ephemeral-test-key".into(),
                base_url: "http://127.0.0.1.invalid".into(),
            }],
            project_root: None,
            hook_bus: Some(Arc::clone(&self.hook_bus)),
        }
    }

    pub async fn interrupt(&self, target: &str) -> anyhow::Result<InterruptAgentV2Result> {
        AgentThreadDispatch::interrupt_agent(
            &self.dispatch_at("/root")?,
            InterruptAgentV2Request {
                target: target.into(),
            },
        )
        .await
    }

    pub async fn close_subtree(&self, target: &str) -> anyhow::Result<AgentTreeSnapshotV2> {
        self.desktop()
            .close_subtree(&self.root_thread_id, target)
            .await
    }

    pub async fn restart_with(
        &mut self,
        script: Vec<ScriptedTurn>,
    ) -> anyhow::Result<Arc<Mutex<Vec<String>>>> {
        anyhow::ensure!(
            self.runtime_manager.active_count() == 0,
            "cannot restart test app with an active runtime"
        );
        let store = AgentGraphStore::open(self.memory_dir.join("subagents-v2.db"))?;
        store.cleanup_pending_reservations(&self.root_thread_id)?;
        store.recover_running_as_interrupted(&self.root_thread_id)?;
        self.control = AgentControl::open(
            self.root_thread_id.clone(),
            store,
            Limits {
                max_threads: 16,
                max_depth: 8,
                max_running: 4,
            },
        )?;
        self.runtime_manager = Arc::new(AgentRuntimeManager::default());
        self.runtime_requests = Arc::new(RuntimeRequestRegistry::default());
        (self.chat_override, self.provider_calls) = scripted_chat(script);
        let previous_hook_events = Arc::clone(&self.hook_events);
        (self.hook_bus, self.hook_events) = lifecycle_hooks();
        let sessions = session::SessionStore::open_sessions_dir(&self.memory_dir.join("sessions"))?;
        anyhow::ensure!(
            sessions.get_session(&self.root_thread_id)?.is_some(),
            "root session disappeared during restart"
        );
        Ok(previous_hook_events)
    }

    pub async fn wait_for_status(
        &self,
        target: &str,
        expected: AgentStatusKind,
    ) -> anyhow::Result<()> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let status = self.status(target)?;
                if status.kind() == expected {
                    return Ok::<_, anyhow::Error>(());
                }
                if matches!(status, AgentStatusV2::Errored { .. }) {
                    anyhow::bail!("agent thread entered Errored while waiting for {expected:?}");
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("timed out waiting for {target} to become {expected:?}"))??;
        Ok(())
    }

    pub fn status(&self, target: &str) -> anyhow::Result<AgentStatusV2> {
        Ok(self.control.resolve_desktop_target(target)?.status)
    }

    pub fn is_running(&self, thread_id: &str) -> bool {
        self.runtime_manager.is_running(thread_id)
    }

    pub fn pending_mailbox(&self, target: &str) -> anyhow::Result<usize> {
        let thread = self.control.resolve_desktop_target(target)?;
        Ok(self.control.drain_mailbox(&thread.canonical_path)?.len())
    }

    pub fn session_contents(&self, target: &str) -> anyhow::Result<Vec<String>> {
        let thread = self.control.resolve_desktop_target(target)?;
        Ok(
            session::SessionStore::open_sessions_dir(&self.memory_dir.join("sessions"))?
                .get_messages(&thread.session_id)?
                .into_iter()
                .filter_map(|message| message.content)
                .collect(),
        )
    }

    pub fn has_runtime_handle(&self, thread_id: &str) -> anyhow::Result<bool> {
        Ok(self.control.runtime_handle(thread_id)?.is_some())
    }

    pub fn identity_count(&self) -> anyhow::Result<usize> {
        self.control.identity_count()
    }

    pub fn active_execution_count(&self) -> anyhow::Result<usize> {
        self.control.active_execution_count()
    }

    pub fn active_runtime_count(&self) -> usize {
        self.runtime_manager.active_count()
    }

    pub fn runtime_request_count(&self) -> usize {
        self.runtime_requests
            .requests
            .lock()
            .map(|requests| requests.len())
            .unwrap_or(usize::MAX)
    }

    pub fn session_row_count(&self) -> anyhow::Result<usize> {
        Ok(self.session_ids()?.len())
    }

    pub fn session_ids(&self) -> anyhow::Result<Vec<String>> {
        let sessions = session::SessionStore::open_sessions_dir(&self.memory_dir.join("sessions"))?;
        Ok(sessions
            .list_sessions(session::SessionListFilter::Active, 100)?
            .into_iter()
            .map(|session| session.id)
            .collect())
    }

    pub fn hook_events(&self) -> Vec<String> {
        self.hook_events.lock().unwrap().clone()
    }

    pub fn provider_calls(&self) -> Vec<CapturedProviderCall> {
        self.provider_calls.lock().unwrap().clone()
    }
}
