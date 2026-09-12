//! 图片生成请求参数与构建器。
//!
//! 封装提示词、供应商、尺寸与数量，供上层统一构造出图任务。

/// 一次图片生成请求的完整参数。
#[derive(Debug, Clone)]
pub struct ImageGenRequest {
    /// 文本提示词。
    pub prompt: String,
    /// 供应商标识符。
    pub provider: String,
    /// 图片模型名称。
    pub model: String,
    /// 图片宽度（像素）。
    pub width: u32,
    /// 图片高度（像素）。
    pub height: u32,
    /// 生成张数。
    pub count: u32,
}

impl ImageGenRequest {
    /// 返回链式构建器。
    pub fn builder() -> ImageGenRequestBuilder {
        ImageGenRequestBuilder::default()
    }
}

pub const IMAGE_MODEL_CHARACTER: &str = "gpt-image-2.5-sunburst";
pub const IMAGE_MODEL_GENERAL: &str = "gpt-image-2.5-flare";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ImageScene {
    #[default]
    Auto,
    Character,
    Animation,
    Wallpaper,
    General,
}

pub fn supports_scene_image_model(provider: &str) -> bool {
    matches!(
        crate::profile::resolve_or_openai_compat(provider).id,
        "openai" | "azure"
    )
}

/// Explicit models (including dated deployments) always win. Blank remains auto
/// until the request's intent/reference inputs are known; never infer from prose.
pub fn select_image_model(
    provider: &str,
    explicit: &str,
    scene: ImageScene,
    has_reference: bool,
) -> String {
    if !explicit.trim().is_empty() {
        return explicit.trim().to_owned();
    }
    if supports_scene_image_model(provider) {
        return match scene {
            ImageScene::Character | ImageScene::Animation => IMAGE_MODEL_CHARACTER,
            ImageScene::Auto if has_reference => IMAGE_MODEL_CHARACTER,
            _ => IMAGE_MODEL_GENERAL,
        }
        .to_owned();
    }
    default_image_model(provider).to_owned()
}

/// 按 provider 返回普通生图默认模型（表驱动）。
pub fn default_image_model(provider: &str) -> &'static str {
    let p = crate::profile::resolve_or_openai_compat(provider);
    if p.default_image_model.is_empty() {
        IMAGE_MODEL_GENERAL
    } else {
        p.default_image_model
    }
}

#[cfg(test)]
mod scene_tests {
    use super::*;
    #[test]
    fn image_scene_defaults_respect_intent_and_explicit_deployments() {
        for provider in ["openai", "azure"] {
            for scene in [ImageScene::Character, ImageScene::Animation] {
                assert_eq!(
                    select_image_model(provider, "", scene, false),
                    IMAGE_MODEL_CHARACTER
                );
            }
            for scene in [ImageScene::Wallpaper, ImageScene::General] {
                assert_eq!(
                    select_image_model(provider, "", scene, true),
                    IMAGE_MODEL_GENERAL
                );
            }
            assert_eq!(
                select_image_model(provider, "", ImageScene::Auto, true),
                IMAGE_MODEL_CHARACTER
            );
            assert_eq!(
                select_image_model(provider, "", ImageScene::Auto, false),
                IMAGE_MODEL_GENERAL
            );
            assert_eq!(
                select_image_model(provider, " custom-deployment ", ImageScene::Character, true),
                "custom-deployment"
            );
        }
        assert_eq!(
            select_image_model("google", "", ImageScene::Character, true),
            default_image_model("google")
        );
        assert_eq!(
            select_image_model("zhipu", "", ImageScene::Character, true),
            default_image_model("zhipu")
        );
    }
}

/// [`ImageGenRequest`] 的链式构建器。
#[derive(Default)]
pub struct ImageGenRequestBuilder {
    /// 提示词。
    prompt: String,
    /// 供应商标识符。
    provider: String,
    /// 模型名称。
    model: String,
    /// 宽度。
    width: u32,
    /// 高度。
    height: u32,
    /// 生成数量。
    count: u32,
}

impl ImageGenRequestBuilder {
    /// 设置提示词。
    pub fn prompt(mut self, p: &str) -> Self {
        self.prompt = p.to_string();
        self
    }

    /// 设置供应商（默认 `google`）。
    pub fn provider(mut self, p: &str) -> Self {
        self.provider = p.to_string();
        self
    }

    /// 设置模型（为空时使用 [`default_image_model`]）。
    pub fn model(mut self, m: &str) -> Self {
        self.model = m.to_string();
        self
    }

    /// 设置图片尺寸（宽 × 高，0 表示使用默认 1024）。
    pub fn size(mut self, w: u32, h: u32) -> Self {
        self.width = w;
        self.height = h;
        self
    }

    /// 设置生成张数（0 表示 1 张）。
    pub fn count(mut self, c: u32) -> Self {
        self.count = c;
        self
    }

    /// 完成构建，填充默认值。
    pub fn build(self) -> ImageGenRequest {
        let provider = if self.provider.is_empty() {
            "google".to_string()
        } else {
            self.provider
        };
        let model = if self.model.is_empty() {
            default_image_model(&provider).to_string()
        } else {
            self.model
        };
        ImageGenRequest {
            prompt: self.prompt,
            provider,
            model,
            width: if self.width == 0 { 1024 } else { self.width },
            height: if self.height == 0 { 1024 } else { self.height },
            count: if self.count == 0 { 1 } else { self.count },
        }
    }
}
