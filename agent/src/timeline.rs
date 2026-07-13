//! 助手回合时间线：交错 reasoning / activity / surface，写入 `reasoning_details`。

use serde_json::{json, Value};

/// 流式拼装 `astro_timeline_v1` / `astro_surfaces_v1`。
#[derive(Debug, Clone, Default)]
pub struct TimelineBuilder {
    segments: Vec<Value>,
    surfaces: Vec<Value>,
}

impl TimelineBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// 追加 reasoning；末段已是 reasoning 则拼接文本。
    pub fn push_reasoning_delta(&mut self, delta: &str, at_ms: i64) {
        if delta.is_empty() {
            return;
        }
        if let Some(last) = self.segments.last_mut() {
            if last.get("type").and_then(|t| t.as_str()) == Some("reasoning") {
                let text = last.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string()
                    + delta;
                last.as_object_mut()
                    .unwrap()
                    .insert("text".into(), json!(text));
                return;
            }
        }
        self.segments.push(json!({
            "type": "reasoning",
            "id": format!("r-{at_ms}-{}", self.segments.len()),
            "text": delta,
            "at": at_ms,
        }));
    }

    /// 新 activity id 时追加段；已存在则忽略。
    pub fn upsert_activity(&mut self, id: &str, at_ms: i64) {
        if id.is_empty() {
            return;
        }
        let exists = self.segments.iter().any(|s| {
            s.get("type").and_then(|t| t.as_str()) == Some("activity")
                && s.get("id").and_then(|t| t.as_str()) == Some(id)
        });
        if !exists {
            self.segments.push(json!({
                "type": "activity",
                "id": id,
                "at": at_ms,
            }));
        }
    }

    /// upsert surface 实体；新 messageId 时追加 surface 段。
    pub fn upsert_surface(&mut self, surface: Value, at_ms: i64) {
        let sid = surface
            .get("messageId")
            .or_else(|| surface.get("message_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if sid.is_empty() {
            return;
        }
        if let Some(pos) = self.surfaces.iter().position(|s| {
            s.get("messageId").and_then(|v| v.as_str()) == Some(sid.as_str())
        }) {
            self.surfaces[pos] = surface;
        } else {
            self.surfaces.push(surface);
            self.segments.push(json!({
                "type": "surface",
                "id": sid,
                "at": at_ms,
            }));
        }
    }

    /// 合并进已有 reasoning_details 对象。
    pub fn into_reasoning_details(self, existing: Option<Value>) -> Value {
        let mut obj = match existing {
            Some(Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        };
        obj.insert("astro_timeline_v1".into(), Value::Array(self.segments));
        if !self.surfaces.is_empty() {
            obj.insert("astro_surfaces_v1".into(), Value::Array(self.surfaces));
        }
        Value::Object(obj)
    }

    /// 当前快照（不消费 builder）。
    pub fn reasoning_details_snapshot(&self) -> Value {
        self.clone().into_reasoning_details(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaves_reasoning_and_activity() {
        let mut b = TimelineBuilder::new();
        b.push_reasoning_delta("a", 1);
        b.upsert_activity("c1", 2);
        b.push_reasoning_delta("b", 3);
        let v = b.into_reasoning_details(None);
        let segs = v["astro_timeline_v1"].as_array().unwrap();
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0]["type"], "reasoning");
        assert_eq!(segs[1]["type"], "activity");
        assert_eq!(segs[2]["text"], "b");
    }

    #[test]
    fn activity_upsert_same_id_no_duplicate() {
        let mut b = TimelineBuilder::new();
        b.upsert_activity("c1", 1);
        b.upsert_activity("c1", 2);
        let v = b.into_reasoning_details(None);
        assert_eq!(v["astro_timeline_v1"].as_array().unwrap().len(), 1);
    }
}
