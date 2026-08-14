//! Agent 运行期事件总线。
//!
//! 基于 Tokio `broadcast` 将 token、工具调用、记忆更新等事件推送给多个订阅方（如前端 SSE）。
//! 发送失败（无订阅者或通道已满）时静默丢弃，避免阻塞 Agent 主循环。

use serde_json::Value;
use tokio::sync::broadcast;

/// Agent 生命周期内可观测的单次事件。
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// LLM 流式输出的文本片段。
    Token(String),
    /// 模型发起工具调用；`args` 为 JSON 形参。
    ToolCall { name: String, args: Value },
    /// 工具执行完毕；`result` 为面向模型的文本摘要。
    ToolResult { name: String, result: String },
    /// 记忆层变更（写入/删除等）；`op` 为操作名，`content` 为变更摘要。
    MemoryUpdate { op: String, content: String },
    /// 本轮对话正常结束。
    Done,
    /// 不可恢复的运行错误描述。
    Error(String),
}

/// 多订阅方共享的事件发布端。
///
/// 容量由构造时指定；慢消费者可能因 lag 而丢失较早事件（broadcast 语义）。
pub struct EventBus {
    /// 底层广播发送器。
    tx: broadcast::Sender<AgentEvent>,
}

impl EventBus {
    /// 创建指定容量的广播通道。
    ///
    /// `capacity` 为环形缓冲区大小；过小会导致高频 token 流时丢事件。
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        EventBus { tx }
    }

    /// 订阅后续事件；每个接收方独立维护游标。
    pub fn subscribe(&self) -> broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    /// 异步发布事件；无活跃订阅者时不阻塞。
    pub async fn publish(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }
}
