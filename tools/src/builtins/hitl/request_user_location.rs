//! 请求用户定位：本地天气/附近查询前向用户授权获取坐标或城市。
//!
//! 返回带 `astro_hitl` 标记的 A2UI JSON；streaming 层 park，前端 Geolocation
//! 或用户填写城市后 resume。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

const DEFAULT_MESSAGE: &str =
    "查询本地天气或附近信息需要你的位置。请授权共享当前位置，或手动填写城市。";

/// Arguments for `request_user_location`.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RequestUserLocationArgs {
    /// Why location is needed (optional; default message when empty).
    #[serde(default)]
    pub message: String,
}

/// 向注册表登记 `request_user_location` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "request_user_location".to_string(),
        toolset: "request_user_location".to_string(),
        description: "Request the user's location before local queries (weather, nearby places). \
Never assume a city. Prefer this over ask(question) for location — resume is one of: \
(1) latitude+longitude (+optional accuracy_m), (2) city string, (3) denied=true. \
Not for mode switching (request_mode_switch) or generic confirmations (ask)."
            .to_string(),
        schema: schema_for_args::<RequestUserLocationArgs>(),
        check_fn: None,
        icon: "map-pin",
        needs_confirmation: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_user_location"],
    sync_ctx: dispatch,
}

/// 构建 HITL 定位请求载荷（A2UI operations + response schema）。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: RequestUserLocationArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("request_user_location 参数无效: {e}"))?;
    let message = {
        let trimmed = parsed.message.trim();
        if trimmed.is_empty() {
            DEFAULT_MESSAGE.to_string()
        } else {
            trimmed.to_string()
        }
    };

    let surface_id = format!("location-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_location_request_surface(&surface_id, &message);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("request_user_location A2UI 无效: {e}"))?;

    let payload = json!({
        "astro_hitl": true,
        "reason": "location_required",
        "message": message,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "description": "Exactly one shape: coords, city, or denied",
            "properties": {
                "latitude": { "type": "number" },
                "longitude": { "type": "number" },
                "accuracy_m": { "type": "number" },
                "city": { "type": "string" },
                "denied": { "type": "boolean" }
            },
            "oneOf": [
                {
                    "required": ["latitude", "longitude"],
                    "description": "Geolocation granted"
                },
                {
                    "required": ["city"],
                    "description": "User typed a city"
                },
                {
                    "required": ["denied"],
                    "properties": { "denied": { "const": true } },
                    "description": "User denied location"
                }
            ]
        }
    });
    Ok(payload.to_string())
}
