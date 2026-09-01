use serde::{Deserialize, Serialize};

mod fork;
mod path;
mod policy;
mod reconstruction;
mod recorder;

pub use fork::*;
pub use path::*;
pub use policy::*;
pub use reconstruction::*;
pub use recorder::*;

#[derive(Debug, Clone, PartialEq)]
pub enum RolloutItem {
    SessionMeta(serde_json::Value),
    ResponseItem(agent_protocol::ResponseItem),
    RealtimeItem(agent_protocol::RealtimeItem),
    EventMsg(agent_protocol::EventMsg),
    TurnContext(serde_json::Value),
    WorldState(serde_json::Value),
    Compacted(serde_json::Value),
    InterAgentCommunication(serde_json::Value),
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RolloutItemWire {
    SessionMeta {
        payload: serde_json::Value,
    },
    ResponseItem {
        payload: agent_protocol::ResponseItem,
    },
    RealtimeItem {
        payload: agent_protocol::RealtimeItem,
    },
    EventMsg {
        payload: agent_protocol::EventMsg,
    },
    TurnContext {
        payload: serde_json::Value,
    },
    WorldState {
        payload: serde_json::Value,
    },
    Compacted {
        payload: serde_json::Value,
    },
    InterAgentCommunication {
        payload: serde_json::Value,
    },
}

impl Serialize for RolloutItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let wire = match self {
            Self::SessionMeta(payload) => RolloutItemWire::SessionMeta {
                payload: payload.clone(),
            },
            Self::ResponseItem(payload) => RolloutItemWire::ResponseItem {
                payload: payload.clone(),
            },
            Self::RealtimeItem(payload) => RolloutItemWire::RealtimeItem {
                payload: payload.clone(),
            },
            Self::EventMsg(payload) => RolloutItemWire::EventMsg {
                payload: payload.clone(),
            },
            Self::TurnContext(payload) => RolloutItemWire::TurnContext {
                payload: payload.clone(),
            },
            Self::WorldState(payload) => RolloutItemWire::WorldState {
                payload: payload.clone(),
            },
            Self::Compacted(payload) => RolloutItemWire::Compacted {
                payload: payload.clone(),
            },
            Self::InterAgentCommunication(payload) => RolloutItemWire::InterAgentCommunication {
                payload: payload.clone(),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RolloutItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(match RolloutItemWire::deserialize(deserializer)? {
            RolloutItemWire::SessionMeta { payload } => Self::SessionMeta(payload),
            RolloutItemWire::ResponseItem { payload } => Self::ResponseItem(payload),
            RolloutItemWire::RealtimeItem { payload } => Self::RealtimeItem(payload),
            RolloutItemWire::EventMsg { payload } => Self::EventMsg(payload),
            RolloutItemWire::TurnContext { payload } => Self::TurnContext(payload),
            RolloutItemWire::WorldState { payload } => Self::WorldState(payload),
            RolloutItemWire::Compacted { payload } => Self::Compacted(payload),
            RolloutItemWire::InterAgentCommunication { payload } => {
                Self::InterAgentCommunication(payload)
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RolloutLine {
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u64>,
    #[serde(flatten)]
    pub item: RolloutItem,
}
