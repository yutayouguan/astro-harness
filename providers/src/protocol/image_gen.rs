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

/// 按 provider 返回默认图片模型。
pub fn default_image_model(provider: &str) -> &'static str {
    match provider {
        "google" => "gemini-3.1-flash-image",
        "openai" => "gpt-image-2",
        "minimax" => "image-01",
        _ => "gpt-image-2",
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
