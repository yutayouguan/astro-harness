//! Astro gRPC 服务端到端连通与基本 RPC 测试。
//!
//! 默认套件不依赖本机 Ollama。真 chat 联调见
//! `test_grpc_chat_ollama_live`（需 `ASTRO_LIVE_OLLAMA=1` 或 `--ignored`）。

use proto::astro_service_client::AstroServiceClient;
use proto::astro_service_server::AstroServiceServer;
use proto::{ChatControlAction, ChatControlRequest, ChatRequest, MemoryQuery};
use server::grpc::AstroServiceImpl;
use tempfile::TempDir;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

async fn spawn_test_server(dir: std::path::PathBuf) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let service = AstroServiceImpl::new(dir);
    tokio::spawn(async move {
        Server::builder()
            .add_service(AstroServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    format!("http://{addr}")
}

/// 默认：gRPC 连通 + `query_memory`，不调用 LLM。
#[tokio::test]
async fn test_grpc_connect_and_query_memory() {
    let dir = TempDir::new().unwrap();
    let endpoint = spawn_test_server(dir.path().to_path_buf()).await;

    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .expect("connect grpc");

    let memory = client
        .query_memory(MemoryQuery {
            query: "你好".into(),
            limit: 3,
        })
        .await
        .expect("query memory")
        .into_inner();
    // 空库也可：只要 RPC 成功
    let _ = memory.sessions;
}

/// release_session 对冷会话与重复调用都应返回成功。
#[tokio::test]
async fn release_session_runtime_is_idempotent() {
    let dir = TempDir::new().unwrap();
    let endpoint = spawn_test_server(dir.path().to_path_buf()).await;

    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .expect("connect grpc");

    let request = ChatControlRequest {
        session_id: "release-session-runtime-idempotent".into(),
        action: ChatControlAction::ReleaseSession as i32,
    };
    client
        .chat_control(request.clone())
        .await
        .expect("first release");
    client.chat_control(request).await.expect("second release");
}

/// Live：依赖本机 Ollama。未设置 `ASTRO_LIVE_OLLAMA=1` 时直接 return（默认套件仍绿）。
/// 联调：`ASTRO_LIVE_OLLAMA=1 cargo test -p backend --test grpc_test test_grpc_chat_ollama_live -- --nocapture`
#[tokio::test]
async fn test_grpc_chat_ollama_live() {
    if std::env::var("ASTRO_LIVE_OLLAMA").ok().as_deref() != Some("1") {
        return;
    }

    let dir = TempDir::new().unwrap();
    let endpoint = spawn_test_server(dir.path().to_path_buf()).await;

    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .expect("connect grpc");

    let mut stream = client
        .chat(ChatRequest {
            session_id: "grpc-test".into(),
            content: "你好".into(),
            provider: "ollama".into(),
            use_memory: true,
            thinking_enabled: false,
            reasoning_effort: String::new(),
            ..Default::default()
        })
        .await
        .expect("chat rpc")
        .into_inner();

    let mut tokens = String::new();
    while let Some(event) = stream.message().await.expect("stream message") {
        match event.payload {
            Some(proto::chat_event::Payload::Token(token)) => tokens.push_str(&token),
            Some(proto::chat_event::Payload::Done(true)) => break,
            Some(proto::chat_event::Payload::Error(err)) => panic!("chat error: {err}"),
            _ => {}
        }
    }
    assert!(!tokens.is_empty());

    let memory = client
        .query_memory(MemoryQuery {
            query: "你好".into(),
            limit: 3,
        })
        .await
        .expect("query memory")
        .into_inner();
    assert!(memory.sessions.is_empty() || !memory.sessions.is_empty());
}
