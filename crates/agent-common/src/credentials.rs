//! 工具执行凭证：LLM 聊天与媒体生成的 Provider 凭证。

/// 当前聊天会话的 LLM 凭证。
#[derive(Debug, Clone, Default)]
pub struct ModelCredentials {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

/// 单个媒体生成 Provider 的调用凭证。
#[derive(Debug, Clone, Default)]
pub struct ImageGenCreds {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    pub video_model: String,
    pub music_model: String,
    pub tts_model: String,
    pub vision_model: String,
}

/// 主备媒体凭证对。
#[derive(Debug, Clone, Default)]
pub struct ImageGenTargets {
    pub primary: Option<ImageGenCreds>,
    pub fallback: Option<ImageGenCreds>,
}

/// `ImageGenTargets::from_parts` 的扁平字符串入参。
#[derive(Debug, Clone, Copy)]
pub struct ImageGenParts<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub api_key: &'a str,
    pub base_url: &'a str,
    pub fb_provider: &'a str,
    pub fb_model: &'a str,
    pub fb_api_key: &'a str,
    pub fb_base_url: &'a str,
    pub video_model: &'a str,
    pub music_model: &'a str,
    pub tts_model: &'a str,
    pub fb_video_model: &'a str,
    pub fb_music_model: &'a str,
    pub fb_tts_model: &'a str,
    pub vision_model: &'a str,
    pub fb_vision_model: &'a str,
}

impl ImageGenTargets {
    /// 从扁平字符串字段构建主备凭证。
    ///
    /// `default_model_fn` 在图片 model 为空时用于获取 Provider 默认模型名。
    pub fn from_parts(p: ImageGenParts<'_>, default_model_fn: impl Fn(&str) -> String) -> Self {
        let primary = if !p.provider.is_empty() && !p.api_key.is_empty() {
            Some(ImageGenCreds {
                provider: p.provider.to_string(),
                model: if p.model.is_empty() {
                    default_model_fn(p.provider)
                } else {
                    p.model.to_string()
                },
                api_key: p.api_key.to_string(),
                base_url: p.base_url.to_string(),
                video_model: p.video_model.trim().to_string(),
                music_model: p.music_model.trim().to_string(),
                tts_model: p.tts_model.trim().to_string(),
                vision_model: p.vision_model.trim().to_string(),
            })
        } else {
            None
        };
        let fallback = if !p.fb_provider.is_empty() && !p.fb_api_key.is_empty() {
            Some(ImageGenCreds {
                provider: p.fb_provider.to_string(),
                model: if p.fb_model.is_empty() {
                    default_model_fn(p.fb_provider)
                } else {
                    p.fb_model.to_string()
                },
                api_key: p.fb_api_key.to_string(),
                base_url: p.fb_base_url.to_string(),
                video_model: p.fb_video_model.trim().to_string(),
                music_model: p.fb_music_model.trim().to_string(),
                tts_model: p.fb_tts_model.trim().to_string(),
                vision_model: p.fb_vision_model.trim().to_string(),
            })
        } else {
            None
        };
        Self { primary, fallback }
    }

    pub fn is_empty(&self) -> bool {
        self.primary.is_none() && self.fallback.is_none()
    }

    pub fn find_provider(&self, provider: &str) -> Option<&ImageGenCreds> {
        self.primary
            .as_ref()
            .filter(|c| c.provider == provider)
            .or_else(|| self.fallback.as_ref().filter(|c| c.provider == provider))
    }

    pub fn google(&self) -> Option<&ImageGenCreds> {
        self.find_provider("google")
    }

    pub fn openai(&self) -> Option<&ImageGenCreds> {
        self.find_provider("openai")
    }

    pub fn minimax(&self) -> Option<&ImageGenCreds> {
        self.find_provider("minimax")
    }
}
