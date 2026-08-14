//! 动态 dispatch wrapper — 将静态泛型模型 trait 包装为 `dyn` trait object。
//!
//! 运行时注册表需要按 string id 查找 provider，无法用泛型，
//! 因此通过 `Dyn*Model` 擦除类型。

use anyhow::Result;
use async_trait::async_trait;

use super::models::*;
use crate::types::media::{
    Embedding, GeneratedAudio, GeneratedImage, GeneratedVideo, ImageGenConfig, MusicGenConfig,
    TTSConfig, VideoGenConfig,
};
use crate::types::{CompletionRequest, CompletionStream};

// ─── 宏：生成 Dyn trait + blanket impl + Box Clone ──────

macro_rules! dyn_model {
    (
        $(#[$meta:meta])*
        $dyn_trait:ident for $model_trait:ident {
            $(async fn $method:ident(&self $(, $arg:ident: $arg_ty:ty)*) -> $ret:ty;)+
        }
    ) => {
        $(#[$meta])*
        #[async_trait]
        pub trait $dyn_trait: Send + Sync {
            $(async fn $method(&self $(, $arg: $arg_ty)*) -> $ret;)+
            fn clone_box(&self) -> Box<dyn $dyn_trait>;
        }

        #[async_trait]
        impl<M: $model_trait + Clone + 'static> $dyn_trait for M {
            $(async fn $method(&self $(, $arg: $arg_ty)*) -> $ret {
                <Self as $model_trait>::$method(self $(, $arg)*).await
            })+

            fn clone_box(&self) -> Box<dyn $dyn_trait> {
                Box::new(self.clone())
            }
        }

        impl Clone for Box<dyn $dyn_trait> {
            fn clone(&self) -> Self {
                self.clone_box()
            }
        }
    };
}

// ─── 各能力的 Dyn trait ─────────────────────────────────

dyn_model! {
    /// 类型擦除的补全模型。
    DynCompletionModel for CompletionModel {
        async fn stream(&self, request: CompletionRequest) -> Result<CompletionStream>;
    }
}

dyn_model! {
    /// 类型擦除的嵌入模型。
    DynEmbeddingModel for EmbeddingModel {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Embedding>>;
    }
}

dyn_model! {
    /// 类型擦除的图片生成模型。
    DynImageGenModel for ImageGenModel {
        async fn generate(&self, prompt: &str, config: &ImageGenConfig) -> Result<Vec<GeneratedImage>>;
    }
}

dyn_model! {
    /// 类型擦除的视频生成模型。
    DynVideoGenModel for VideoGenModel {
        async fn generate(&self, prompt: &str, config: &VideoGenConfig) -> Result<GeneratedVideo>;
    }
}

dyn_model! {
    /// 类型擦除的语音合成模型。
    DynTTSModel for TTSModel {
        async fn synthesize(&self, text: &str, config: &TTSConfig) -> Result<GeneratedAudio>;
    }
}

dyn_model! {
    /// 类型擦除的音乐生成模型。
    DynMusicGenModel for MusicGenModel {
        async fn generate(&self, prompt: &str, config: &MusicGenConfig) -> Result<GeneratedAudio>;
    }
}

// ─── DynProvider ────────────────────────────────────────

/// 动态 provider entry — 注册表中的一项，包含所有能力的可选 dyn trait object。
#[derive(Clone)]
pub struct DynProvider {
    pub id: String,
    pub name: &'static str,
    completion: Option<Box<dyn DynCompletionModel>>,
    embedding: Option<Box<dyn DynEmbeddingModel>>,
    image_gen: Option<Box<dyn DynImageGenModel>>,
    video_gen: Option<Box<dyn DynVideoGenModel>>,
    tts: Option<Box<dyn DynTTSModel>>,
    music_gen: Option<Box<dyn DynMusicGenModel>>,
}

impl DynProvider {
    pub fn new(id: impl Into<String>, name: &'static str) -> Self {
        Self {
            id: id.into(),
            name,
            completion: None,
            embedding: None,
            image_gen: None,
            video_gen: None,
            tts: None,
            music_gen: None,
        }
    }

    pub fn with_completion(mut self, model: impl CompletionModel + Clone + 'static) -> Self {
        self.completion = Some(Box::new(model));
        self
    }

    pub fn with_embedding(mut self, model: impl EmbeddingModel + Clone + 'static) -> Self {
        self.embedding = Some(Box::new(model));
        self
    }

    pub fn with_image_gen(mut self, model: impl ImageGenModel + Clone + 'static) -> Self {
        self.image_gen = Some(Box::new(model));
        self
    }

    pub fn with_video_gen(mut self, model: impl VideoGenModel + Clone + 'static) -> Self {
        self.video_gen = Some(Box::new(model));
        self
    }

    pub fn with_tts(mut self, model: impl TTSModel + Clone + 'static) -> Self {
        self.tts = Some(Box::new(model));
        self
    }

    pub fn with_music_gen(mut self, model: impl MusicGenModel + Clone + 'static) -> Self {
        self.music_gen = Some(Box::new(model));
        self
    }

    pub fn completion_model(&self) -> Option<&dyn DynCompletionModel> {
        self.completion.as_deref()
    }

    pub fn embedding_model(&self) -> Option<&dyn DynEmbeddingModel> {
        self.embedding.as_deref()
    }

    pub fn image_gen_model(&self) -> Option<&dyn DynImageGenModel> {
        self.image_gen.as_deref()
    }

    pub fn video_gen_model(&self) -> Option<&dyn DynVideoGenModel> {
        self.video_gen.as_deref()
    }

    pub fn tts_model(&self) -> Option<&dyn DynTTSModel> {
        self.tts.as_deref()
    }

    pub fn music_gen_model(&self) -> Option<&dyn DynMusicGenModel> {
        self.music_gen.as_deref()
    }

    pub fn has_completion(&self) -> bool {
        self.completion.is_some()
    }
}

impl std::fmt::Debug for DynProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynProvider")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("has_completion", &self.completion.is_some())
            .field("has_embedding", &self.embedding.is_some())
            .field("has_image_gen", &self.image_gen.is_some())
            .field("has_video_gen", &self.video_gen.is_some())
            .field("has_tts", &self.tts.is_some())
            .field("has_music_gen", &self.music_gen.is_some())
            .finish()
    }
}
