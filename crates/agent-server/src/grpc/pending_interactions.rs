//! Global Desktop view backed by live gates/brokers, not historical UI messages.
use super::astro_service::AstroServiceImpl;
use serde_json::{json, Value};
use tonic::{Request, Status};
use types::pending_interaction::*;

fn supported_schema(schema: &Value) -> bool {
    let Some(object) = schema.as_object() else {
        return schema.is_null() || schema.is_boolean();
    };
    // Fail closed for constraints the shared validator does not implement.
    // An allow-list also covers future JSON Schema keywords without silently
    // accepting an answer that violates the producer's original contract.
    if object.keys().any(|key| {
        ![
            "$schema",
            "$id",
            "title",
            "description",
            "default",
            "examples",
            "type",
            "enum",
            "const",
            "properties",
            "required",
            "items",
            "additionalProperties",
            "minLength",
            "maxLength",
            "minimum",
            "maximum",
        ]
        .contains(&key.as_str())
    }) || object.get("type").is_some_and(|value| {
        !value.as_str().is_some_and(|kind| {
            [
                "object", "array", "string", "number", "integer", "boolean", "null",
            ]
            .contains(&kind)
        })
    }) || object
        .get("additionalProperties")
        .is_some_and(|value| !value.is_boolean())
    {
        return false;
    }
    object
        .get("properties")
        .and_then(Value::as_object)
        .is_none_or(|props| props.values().all(supported_schema))
        && object.get("items").is_none_or(supported_schema)
}

fn response_payload(
    request: &PendingInteraction,
    input: &InteractionResponse,
) -> Result<Value, Status> {
    if request.key != input.key
        || request.session_id != input.session_id
        || request.turn_id != input.turn_id
    {
        return Err(Status::failed_precondition("请求绑定已失效"));
    }
    if matches!(request.kind.as_str(), "external" | "unsupported") {
        return Err(Status::failed_precondition("请到会话处理此请求"));
    }
    if !supported_schema(&request.response_schema) {
        return Err(Status::failed_precondition("此表单需要完整会话处理"));
    }
    if request.kind == "approval" {
        let action = request
            .actions
            .iter()
            .find(|a| a.id == input.action)
            .ok_or_else(|| Status::invalid_argument("不允许的授权选项"))?;
        if action.persistent && !input.confirmed_persistent {
            return Err(Status::invalid_argument("长期授权需要明确二次确认"));
        }
        return Ok(action.payload.clone());
    }
    if !["submit", "decline", "cancel"].contains(&input.action.as_str()) {
        return Err(Status::invalid_argument("无效回答操作"));
    }
    if input.action == "submit" {
        if !supported_schema(&request.response_schema) {
            return Err(Status::failed_precondition("此表单需要完整会话处理"));
        }
        agent::control::schema_validate::validate_against_schema(
            &request.response_schema,
            &input.payload,
        )
        .map_err(Status::invalid_argument)?;
    }
    Ok(input.payload.clone())
}

pub async fn snapshot(service: &AstroServiceImpl) -> Result<InteractionSnapshot, Status> {
    let mut cached = service.interaction_snapshot.lock().await;
    let store = session::SessionStore::open_sessions_dir(&home::sessions_dir(&service.memory_dir))
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let mut tasks = Vec::new();
    let mut requests = Vec::new();
    let mut runtimes = std::collections::BTreeMap::new();
    for (id, thread) in service.threads.live_entries().await {
        let turn_id = match thread.runtime.status() {
            agent::AgentStatus::Running { turn_id } => turn_id,
            _ => continue,
        };
        let gate = service.hitl_registry.get(&id).await;
        runtimes.insert(id, (thread.runtime.session().clone(), gate, turn_id));
    }
    for live in agent::runtime::live_interactions::active(&service.memory_dir).await {
        runtimes.insert(
            live.session.session_id().to_string(),
            (live.session, live.gate, live.turn_id),
        );
    }
    for (id, (session, gate, turn_id)) in runtimes {
        let meta = store
            .get_session(&id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        tasks.push(InteractionTask {
            session_id: id.clone(),
            turn_id: turn_id.clone(),
            title: meta
                .as_ref()
                .and_then(|m| m.title.clone())
                .unwrap_or_else(|| "未命名任务".into()),
            parent_session_id: meta.as_ref().and_then(|m| m.parent_session_id.clone()),
            project: session
                .project_root_snapshot()
                .await
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            status: "running".into(),
        });
        if let Some(gate) = gate {
            for pending in gate.pending_interrupts().await {
                let metadata: Value =
                    serde_json::from_str(&pending.metadata_json).unwrap_or(Value::Null);
                // Requests without the actual turn binding cannot be answered here.
                let binding = metadata.get("turnId").and_then(Value::as_str);
                if binding.is_some_and(|bound| bound != turn_id) {
                    continue;
                }
                if chrono::DateTime::parse_from_rfc3339(&pending.expires_at)
                    .is_ok_and(|t| t <= chrono::Utc::now())
                {
                    continue;
                }
                let operations = metadata.get("operations").cloned().unwrap_or(Value::Null);
                let mut actions = approval_actions(&operations);
                let response_schema =
                    serde_json::from_str(&pending.response_schema_json).unwrap_or(Value::Null);
                if actions.iter().any(|a| a.id == "allow_once") {
                    for action in &mut actions {
                        if action.id == "deny" {
                            action.payload = json!({"scope":"deny"});
                        }
                    }
                }
                actions.retain(|action| {
                    agent::control::schema_validate::validate_against_schema(
                        &response_schema,
                        &action.payload,
                    )
                    .is_ok()
                });
                let kind = if binding.is_none() || !supported_schema(&response_schema) {
                    "unsupported"
                } else if !actions.is_empty() {
                    "approval"
                } else if pending.reason == "confirmation"
                    || pending.reason.contains("approval")
                    || !supported_schema(&response_schema)
                {
                    "unsupported"
                } else {
                    "question"
                };
                requests.push(PendingInteraction {
                    key: format!("{}:hitl:{}", cached.epoch, json!([id, turn_id, pending.id])),
                    session_id: id.clone(),
                    turn_id: turn_id.clone(),
                    request_id: pending.id,
                    tool_call_id: pending.tool_call_id,
                    kind: kind.into(),
                    message: pending.message,
                    operations,
                    response_schema,
                    actions,
                    expires_at: pending.expires_at,
                    server_name: None,
                    generation: None,
                });
            }
        }
        for (pending, bound_turn) in session.interaction_broker().pending_requests() {
            if bound_turn.as_deref() != Some(turn_id.as_str()) {
                continue;
            }
            let kind = if pending.params.get("mode").and_then(Value::as_str) == Some("url") {
                "external"
            } else if !supported_schema(
                pending
                    .params
                    .get("requestedSchema")
                    .unwrap_or(&Value::Null),
            ) {
                "unsupported"
            } else {
                "question"
            };
            requests.push(PendingInteraction {
                key: format!(
                    "{}:mcp:{}",
                    cached.epoch,
                    json!([
                        id,
                        turn_id,
                        pending.server_name,
                        pending.request_id,
                        pending.generation
                    ])
                ),
                session_id: id.clone(),
                turn_id: turn_id.clone(),
                request_id: pending.request_id.clone(),
                tool_call_id: format!(
                    "mcp-elicitation:{}:{}",
                    pending.server_name, pending.request_id
                ),
                kind: kind.into(),
                message: pending
                    .params
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("MCP 请求输入")
                    .into(),
                operations: Value::Null,
                response_schema: pending
                    .params
                    .get("requestedSchema")
                    .cloned()
                    .unwrap_or(Value::Null),
                actions: vec![],
                expires_at: String::new(),
                server_name: Some(pending.server_name),
                generation: Some(pending.generation),
            });
        }
    }
    tasks.sort_by(|a, b| a.session_id.cmp(&b.session_id));
    requests.sort_by(|a, b| a.key.cmp(&b.key));
    for task in &mut tasks {
        if requests.iter().any(|r| r.session_id == task.session_id) {
            task.status = "waiting".into();
        }
    }
    if tasks != cached.tasks || requests != cached.requests {
        cached.revision = cached
            .revision
            .checked_add(1)
            .ok_or_else(|| Status::internal("revision exhausted"))?;
        cached.tasks = tasks;
        cached.requests = requests;
    }
    Ok(cached.clone())
}

pub fn encode(snapshot: InteractionSnapshot) -> Result<proto::PendingInteractionsJson, Status> {
    Ok(proto::PendingInteractionsJson {
        json: serde_json::to_string(&snapshot).map_err(|e| Status::internal(e.to_string()))?,
    })
}

pub async fn respond(
    service: &AstroServiceImpl,
    input: InteractionResponse,
) -> Result<InteractionSnapshot, Status> {
    let current = snapshot(service).await?;
    let request = current
        .requests
        .iter()
        .find(|r| {
            r.key == input.key && r.session_id == input.session_id && r.turn_id == input.turn_id
        })
        .ok_or_else(|| Status::failed_precondition("请求已处理、过期或不属于当前回合"))?;
    let session = if let Some(live) =
        agent::runtime::live_interactions::find(&service.memory_dir, &input.session_id).await
    {
        live.session
    } else {
        service
            .threads
            .get(&input.session_id)
            .await
            .ok_or_else(|| Status::failed_precondition("会话已结束"))?
            .runtime
            .session()
            .clone()
    };
    if session.current_turn_id().await.as_deref() != Some(input.turn_id.as_str()) {
        return Err(Status::failed_precondition("回合已变化"));
    }
    let payload = response_payload(request, &input)?;
    if let Some(server) = &request.server_name {
        let action = match input.action.as_str() {
            "submit" => mcp::McpElicitationAction::Accept,
            "decline" => mcp::McpElicitationAction::Decline,
            "cancel" => mcp::McpElicitationAction::Cancel,
            _ => return Err(Status::invalid_argument("无效 MCP 操作")),
        };
        if !session.interaction_broker().resolve_generation(
            server,
            &request.request_id,
            request.generation.unwrap_or_default(),
            mcp::McpElicitationResponse {
                action,
                content: Some(payload),
                meta: None,
            },
        ) {
            return Err(Status::failed_precondition("请求已失效"));
        }
    } else {
        // The existing RPC consumes exactly this live waiter and maintains its
        // existing sidecar/audit lifecycle; it never creates another turn.
        use proto::astro_service_server::AstroService;
        service
            .interrupt_resume(Request::new(proto::InterruptResumeRequest {
                session_id: input.session_id.clone(),
                resume: vec![proto::InterruptResumeItem {
                    interrupt_id: request.request_id.clone(),
                    status: if input.action == "cancel" || input.action == "decline" {
                        "cancelled"
                    } else {
                        "resolved"
                    }
                    .into(),
                    payload_json: payload.to_string(),
                }],
            }))
            .await?;
    }
    snapshot(service).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> PendingInteraction {
        PendingInteraction {
            key: "key".into(),
            session_id: "session".into(),
            turn_id: "turn".into(),
            request_id: "id".into(),
            tool_call_id: "call".into(),
            kind: "approval".into(),
            message: "approve".into(),
            operations: Value::Null,
            response_schema: json!({"type":"object","properties":{"approved":{"type":"boolean"}}}),
            actions: approval_actions(&json!({"variant":"approval","allowAlways":true})),
            expires_at: String::new(),
            server_name: None,
            generation: None,
        }
    }
    fn response(action: &str) -> InteractionResponse {
        InteractionResponse {
            key: "key".into(),
            session_id: "session".into(),
            turn_id: "turn".into(),
            action: action.into(),
            payload: json!({"approved":true,"always":true,"scope":"type"}),
            confirmed_persistent: false,
        }
    }
    #[test]
    fn long_permissions_need_confirmation_and_client_cannot_expand_once_scope() {
        assert!(response_payload(&request(), &response("approve_always")).is_err());
        assert_eq!(
            response_payload(&request(), &response("approve")).unwrap(),
            json!({"approved":true})
        );
        assert!(response_payload(&request(), &response("approve_type")).is_err());
        let mut input = response("approve_always");
        input.confirmed_persistent = true;
        assert!(response_payload(&request(), &input).is_ok());
        input.turn_id = "other".into();
        assert!(response_payload(&request(), &input).is_err());
    }
    #[test]
    fn questions_keep_schema_constraints_and_external_flows_fail_closed() {
        let mut req = request();
        req.kind = "question".into();
        req.response_schema = json!({"type":"object","required":["choice"],"additionalProperties":false,"properties":{"choice":{"type":"string","enum":["a","b"]}}});
        let mut input = response("submit");
        input.payload = json!({"choice":"c"});
        assert!(response_payload(&req, &input).is_err());
        input.payload = json!({"choice":"a"});
        assert!(response_payload(&req, &input).is_ok());
        req.kind = "external".into();
        assert!(response_payload(&req, &input).is_err());
    }

    #[test]
    fn unimplemented_schema_constraints_never_offer_inline_permission() {
        for schema in [
            json!({"type":"number","exclusiveMinimum":0}),
            json!({"type":["string","null"]}),
            json!({"type":"object","additionalProperties":{"type":"string"}}),
            json!({"type":"array","minItems":2}),
            json!({"type":"object","properties":{"answer":{"type":"string","pattern":"^yes$"}}}),
        ] {
            assert!(!supported_schema(&schema));
            let mut req = request();
            req.response_schema = schema;
            assert!(response_payload(&req, &response("approve")).is_err());
        }
    }

    #[tokio::test]
    async fn live_background_requests_resolve_individually_without_current_ui_session() {
        let dir = tempfile::tempdir().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session = std::sync::Arc::new(
            agent::Session::with_session_id(
                agent::runtime::Config::with_defaults(dir.path().to_path_buf()),
                "background-child".into(),
            )
            .await
            .unwrap(),
        );
        session.set_current_turn_id("turn-live").await;
        let gate = agent::HitlGate::new("background-child");
        agent::runtime::live_interactions::register(&session, Some(&gate), "turn-live");
        let mut receivers = vec![];
        for id in ["first", "second"] {
            receivers.push(gate.begin_wait(agent::Interrupt {id:id.into(),reason:"confirmation".into(),message:"Approve read".into(),
                response_schema_json:json!({"type":"object","properties":{"approved":{"type":"boolean"}},"required":["approved"]}).to_string(),
                metadata_json:json!({"turnId":"turn-live","operations":[{"updateComponents":{"components":[{"variant":"approval","allowAlways":false}]}}]}).to_string(),..Default::default()}).await);
        }
        let first = snapshot(&service).await.unwrap();
        assert_eq!(first.requests.len(), 2);
        assert_eq!(first.tasks.len(), 1);
        let selected = first
            .requests
            .iter()
            .find(|r| r.request_id == "first")
            .unwrap();
        let input = InteractionResponse {
            key: selected.key.clone(),
            session_id: selected.session_id.clone(),
            turn_id: selected.turn_id.clone(),
            action: "approve".into(),
            payload: json!({"approved":true,"always":true}),
            confirmed_persistent: false,
        };
        let next = respond(&service, input.clone()).await.unwrap();
        assert_eq!(next.requests.len(), 1);
        assert_eq!(next.requests[0].request_id, "second");
        assert_eq!(
            receivers[0].try_recv().unwrap().payload_json,
            "{\"approved\":true}"
        );
        assert!(respond(&service, input).await.is_err());
        session.clear_current_turn_id().await;
        assert!(snapshot(&service).await.unwrap().requests.is_empty());
    }

    #[tokio::test]
    async fn cross_session_responses_cannot_consume_another_sessions_request() {
        let dir = tempfile::tempdir().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let mut fixtures = vec![];
        for id in ["first-session", "second-session"] {
            let session = std::sync::Arc::new(
                agent::Session::with_session_id(
                    agent::runtime::Config::with_defaults(dir.path().to_path_buf()),
                    id.into(),
                )
                .await
                .unwrap(),
            );
            session.set_current_turn_id("turn").await;
            let gate = agent::HitlGate::new(id);
            agent::runtime::live_interactions::register(&session, Some(&gate), "turn");
            let receiver = gate.begin_wait(agent::Interrupt {
                id: "same-request-id".into(), reason: "question".into(),
                response_schema_json: json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"]}).to_string(),
                metadata_json: json!({"turnId":"turn"}).to_string(), ..Default::default()
            }).await;
            fixtures.push((session, gate, receiver));
        }
        let requests = snapshot(&service).await.unwrap().requests;
        assert_eq!(requests.len(), 2);
        let first = requests
            .iter()
            .find(|r| r.session_id == "first-session")
            .unwrap();
        let mut input = InteractionResponse {
            key: first.key.clone(),
            session_id: "second-session".into(),
            turn_id: "turn".into(),
            action: "submit".into(),
            payload: json!({"answer":"chosen"}),
            confirmed_persistent: false,
        };
        assert!(respond(&service, input.clone()).await.is_err());
        input.session_id = "first-session".into();
        let remaining = respond(&service, input).await.unwrap();
        assert_eq!(remaining.requests.len(), 1);
        assert_eq!(remaining.requests[0].session_id, "second-session");
        assert!(fixtures[0].2.try_recv().is_ok());
        assert!(fixtures[1].2.try_recv().is_err());
    }

    #[tokio::test]
    async fn mcp_request_reuse_cannot_accept_a_stale_generation() {
        let dir = tempfile::tempdir().unwrap();
        memory::ensure_workspace(dir.path()).unwrap();
        let service = AstroServiceImpl::new(dir.path().to_path_buf());
        let session = std::sync::Arc::new(
            agent::Session::with_session_id(
                agent::runtime::Config::with_defaults(dir.path().to_path_buf()),
                "mcp-child".into(),
            )
            .await
            .unwrap(),
        );
        session.set_current_turn_id("turn-live").await;
        let gate = agent::HitlGate::new("mcp-child");
        agent::runtime::live_interactions::register(&session, Some(&gate), "turn-live");
        let broker = session.interaction_broker().clone();
        let queue = broker.requests();
        let start = |broker: std::sync::Arc<mcp::McpElicitationBroker>| {
            tokio::spawn(async move {
                broker.request("server".into(),"request".into(),json!({"requestedSchema":{"type":"object","properties":{"choice":{"type":"string","enum":["a","b"]}},"required":["choice"]}}),tokio_util::sync::CancellationToken::new()).await
            })
        };
        let first = start(broker.clone());
        let event = queue.recv().await.unwrap();
        broker.bind_turn("server", "request", event.generation, "turn-live".into());
        let request = snapshot(&service).await.unwrap().requests.remove(0);
        let input = InteractionResponse {
            key: request.key,
            session_id: request.session_id,
            turn_id: request.turn_id,
            action: "submit".into(),
            payload: json!({"choice":"a"}),
            confirmed_persistent: false,
        };
        respond(&service, input.clone()).await.unwrap();
        first.await.unwrap().unwrap();
        let second = start(broker.clone());
        let event = queue.recv().await.unwrap();
        broker.bind_turn("server", "request", event.generation, "turn-live".into());
        assert!(respond(&service, input).await.is_err());
        assert_eq!(broker.pending_requests().len(), 1);
        second.abort();
        let _ = second.await;
        assert!(snapshot(&service).await.unwrap().requests.is_empty());
    }
}

pub fn watch(
    service: AstroServiceImpl,
) -> tokio_stream::wrappers::ReceiverStream<Result<proto::PendingInteractionsJson, Status>> {
    let (tx, rx) = tokio::sync::mpsc::channel(8);
    let mut changes = types::pending_interaction::changes();
    tokio::spawn(async move {
        let mut previous = None;
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            let value = match snapshot(&service).await {
                Ok(value) => value,
                Err(e) => {
                    let _ = tx.send(Err(e)).await;
                    break;
                }
            };
            if previous != Some(value.revision) {
                previous = Some(value.revision);
                if tx.send(encode(value)).await.is_err() {
                    break;
                }
            }
            tokio::select! { _=tx.closed()=>break, result=changes.changed()=>{if result.is_err(){break;}}, _=tick.tick()=>{} }
        }
    });
    tokio_stream::wrappers::ReceiverStream::new(rx)
}
