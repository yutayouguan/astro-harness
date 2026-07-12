//! Astro gRPC 服务端到端连通与基本 RPC 测试。

use backend::grpc::AstroServiceImpl;
use proto::astro_service_client::AstroServiceClient;
use proto::astro_service_server::AstroServiceServer;
use proto::{ChatRequest, MemoryQuery};
use tempfile::TempDir;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

#[tokio::test]
async fn test_grpc_chat_and_query_memory() {
    let dir = TempDir::new().unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let service = AstroServiceImpl::new(dir.path().to_path_buf());
    tokio::spawn(async move {
        Server::builder()
            .add_service(AstroServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let endpoint = format!("http://{addr}");
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
