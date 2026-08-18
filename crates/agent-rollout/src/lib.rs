use serde::{Deserialize, Serialize};

mod path;
mod policy;
mod reconstruction;
mod recorder;

pub use path::*;
pub use policy::*;
pub use reconstruction::*;
pub use recorder::*;

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
