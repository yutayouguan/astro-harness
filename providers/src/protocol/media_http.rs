//! 媒体 HTTP 兼容 facade：聚合 Google Veo 与 OpenAI Whisper/视觉。
//!
//! 物理实现见 [`crate::google::veo_http`] 与 [`crate::openai::media_compat`]。
//! 新代码优先直接引用厂商子模块；本 facade 仅保持旧路径稳定。

pub use crate::google::veo_http::*;
pub use crate::openai::media_compat::*;

/// 默认视觉（图片理解）模型：按 provider id 转发到各厂商常量。
pub fn default_vision_model(provider: &str) -> &'static str {
    match provider {
        "google" => crate::google::DEFAULT_VISION_MODEL,
        "openai" => crate::openai::DEFAULT_VISION_MODEL,
        _ => crate::openai::DEFAULT_VISION_MODEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_vision_models() {
        assert_eq!(default_vision_model("google"), "gemini-3.5-flash");
        assert_eq!(default_vision_model("openai"), "gpt-4o");
    }
}
