//! 媒体生成结果类型。

/// 生成的图片。
#[derive(Debug, Clone)]
pub struct GeneratedImage {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// 图片生成/编辑请求中的参考图。
#[derive(Debug, Clone)]
pub struct ImageInput {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub filename: String,
}

/// 生成的音频。
#[derive(Debug, Clone)]
pub struct GeneratedAudio {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub duration_ms: u64,
}

/// 生成的视频。
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub width: u32,
    pub height: u32,
}

/// 嵌入向量。
#[derive(Debug, Clone)]
pub struct Embedding {
    pub values: Vec<f32>,
}

/// 图片生成配置。
#[derive(Debug, Clone, Default)]
pub struct ImageGenConfig {
    pub model: String,
    pub scene: super::image_gen::ImageScene,
    pub aspect_ratio: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub n: u32,
    pub output_format: Option<String>,
    pub output_compression: Option<u8>,
    pub quality: Option<String>,
    pub background: Option<String>,
    pub input_images: Vec<ImageInput>,
    pub additional_params: serde_json::Value,
}

/// TTS 配置。
#[derive(Debug, Clone, Default)]
pub struct TTSConfig {
    pub model: String,
    pub voice_id: String,
    pub speed: f32,
    pub additional_params: serde_json::Value,
}

/// 视频生成配置。
#[derive(Debug, Clone, Default)]
pub struct VideoGenConfig {
    pub model: String,
    pub duration_seconds: u32,
    pub resolution: String,
    pub first_frame_image: Option<String>,
    pub last_frame_image: Option<String>,
    pub additional_params: serde_json::Value,
}

/// 音乐生成配置。
#[derive(Debug, Clone, Default)]
pub struct MusicGenConfig {
    pub model: String,
    pub lyrics: Option<String>,
    pub is_instrumental: bool,
    pub additional_params: serde_json::Value,
}
