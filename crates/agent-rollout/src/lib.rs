use serde::{Deserialize, Serialize};

mod policy;

pub use policy::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHistoryMode {
    Legacy,
    Paginated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

impl PartialEq for RolloutItem {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::SessionMeta(a), Self::SessionMeta(b))
            | (Self::TurnContext(a), Self::TurnContext(b))
            | (Self::WorldState(a), Self::WorldState(b))
            | (Self::Compacted(a), Self::Compacted(b))
            | (Self::InterAgentCommunication(a), Self::InterAgentCommunication(b)) => a == b,
            (Self::ResponseItem(a), Self::ResponseItem(b)) => {
                serde_json::to_value(a).ok() == serde_json::to_value(b).ok()
            }
            (Self::EventMsg(a), Self::EventMsg(b)) => a == b,
            _ => false,
        }
    }
}
