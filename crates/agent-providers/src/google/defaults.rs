//! Google Gemini 默认模型与基址常量（单点维护，避免各处硬编码漂移）。

/// 聊天 / 视觉 / 音频理解等通用默认模型。
pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";

/// 视觉（图片理解）默认模型。
pub const DEFAULT_VISION_MODEL: &str = DEFAULT_MODEL;

/// 原生 API 默认 host（无 path）。
pub const DEFAULT_API_HOST: &str = "https://generativelanguage.googleapis.com";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_google_profile() {
        let p = crate::profile::resolve("google").expect("google profile");
        assert_eq!(p.default_model, DEFAULT_MODEL);
        assert_eq!(p.default_base_url, DEFAULT_API_HOST);
    }
}
