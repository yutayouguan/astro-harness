//! 媒体 HTTP 工具函数：视觉/语音/视频生成的跨厂商入口。

pub use crate::google::veo_http::*;
pub use crate::openai::media_compat::*;

/// 默认视觉（图片理解）模型（表驱动）。
pub fn default_vision_model(provider: &str) -> &'static str {
    let p = crate::profile::resolve_or_openai_compat(provider);
    if p.default_vision_model.is_empty() {
        crate::openai::DEFAULT_VISION_MODEL
    } else {
        p.default_vision_model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_vision_models() {
        assert_eq!(default_vision_model("google"), "gemini-3.6-flash");
        assert_eq!(default_vision_model("openai"), "gpt-4o");
    }
}
