use serde::{Deserialize, Serialize};

mod policy;

pub use policy::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHistoryMode {
    Legacy,
    Paginated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RolloutItem {
    SessionMeta(serde_json::Value),
    ResponseItem(types::message::Message),
    EventMsg(agent_protocol::EventMsg),
    TurnContext(serde_json::Value),
    WorldState(serde_json::Value),
    Compacted(serde_json::Value),
    InterAgentCommunication(serde_json::Value),
}
