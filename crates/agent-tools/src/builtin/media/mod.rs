//! 媒体相关：出图 / 视频 / 语音 / 音乐生成 / 视觉 / 音频理解。

pub(super) const MEDIA_GENERATION_NAMESPACE: &str = "media";

pub mod audio_understand;
pub mod image_gen;
pub mod image_understand;
pub(crate) mod media_out;
pub mod music_gen;
pub mod robotics;
pub mod tts;
pub mod video_gen;
pub mod video_understand;
