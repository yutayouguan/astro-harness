//! 泛型客户端 `Client<Ext>` + blanket impl 能力绑定。

use reqwest::{header::HeaderMap, Client as HttpClient};

use super::capability::{Capabilities, Capable};
use super::models::*;

/// 厂商扩展标记 — 每个厂商实现此 trait。
pub trait ProviderExt: Send + Sync + Clone + 'static {
    /// 厂商标识符（如 `"openai"`、`"anthropic"`）。
    const NAME: &'static str;

    /// 默认 API 基址。
    const BASE_URL: &'static str;

    /// 构造认证 header。
    fn auth_headers(&self, api_key: &str) -> HeaderMap;
}

/// 泛型供应商客户端。
///
/// `Ext` 为零大小的厂商标记类型，携带编译期行为。
#[derive(Clone)]
pub struct ProviderClient<Ext> {
    pub(crate) http: HttpClient,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) ext: Ext,
}

impl<Ext: ProviderExt> ProviderClient<Ext> {
    pub fn new(api_key: impl Into<String>, ext: Ext) -> Self {
        Self {
            http: HttpClient::new(),
            base_url: Ext::BASE_URL.to_string(),
            api_key: api_key.into(),
            ext,
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let url = base_url.into();
        if !url.trim().is_empty() {
            self.base_url = url;
        }
        self
    }

    pub fn with_http_client(mut self, client: HttpClient) -> Self {
        self.http = client;
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub fn ext(&self) -> &Ext {
        &self.ext
    }
}

// ─── 能力绑定（blanket impls）────────────────────────────

/// 拥有聊天能力的客户端。
pub trait ChatClient {
    type Model: CompletionModel;
    fn completion_model(&self, model: &str) -> Self::Model;
}

/// blanket impl：当 Ext 声明 Chat = Capable<M> 时，Client<Ext> 自动实现 ChatClient。
impl<Ext, M> ChatClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<Chat = Capable<M>>,
    M: CompletionModel + FromClient<Ext>,
{
    type Model = M;
    fn completion_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 拥有嵌入能力的客户端。
pub trait EmbedClient {
    type Model: EmbeddingModel;
    fn embedding_model(&self, model: &str) -> Self::Model;
}

impl<Ext, M> EmbedClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<Embedding = Capable<M>>,
    M: EmbeddingModel + FromClient<Ext>,
{
    type Model = M;
    fn embedding_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 拥有图片生成能力的客户端。
pub trait ImageGenClient {
    type Model: ImageGenModel;
    fn image_model(&self, model: &str) -> Self::Model;
}

impl<Ext, M> ImageGenClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<ImageGen = Capable<M>>,
    M: ImageGenModel + FromClient<Ext>,
{
    type Model = M;
    fn image_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 拥有 TTS 能力的客户端。
pub trait TTSClient {
    type Model: TTSModel;
    fn tts_model(&self, model: &str) -> Self::Model;
}

impl<Ext, M> TTSClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<TTS = Capable<M>>,
    M: TTSModel + FromClient<Ext>,
{
    type Model = M;
    fn tts_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 拥有视频生成能力的客户端。
pub trait VideoGenClient {
    type Model: VideoGenModel;
    fn video_model(&self, model: &str) -> Self::Model;
}

impl<Ext, M> VideoGenClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<VideoGen = Capable<M>>,
    M: VideoGenModel + FromClient<Ext>,
{
    type Model = M;
    fn video_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 拥有音乐生成能力的客户端。
pub trait MusicGenClient {
    type Model: MusicGenModel;
    fn music_model(&self, model: &str) -> Self::Model;
}

impl<Ext, M> MusicGenClient for ProviderClient<Ext>
where
    Ext: ProviderExt + Capabilities<MusicGen = Capable<M>>,
    M: MusicGenModel + FromClient<Ext>,
{
    type Model = M;
    fn music_model(&self, model: &str) -> M {
        M::from_client(self, model)
    }
}

/// 从 `ProviderClient<Ext>` 构造模型实例。
pub trait FromClient<Ext: ProviderExt>: Sized {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self;
}

// ─── 通用模型基座 ───────────────────────────────────────

/// 多模态模型共用的 HTTP 连接字段。
///
/// 所有非 Chat 模型（Embedding / ImageGen / TTS / VideoGen / MusicGen）
/// 结构完全相同：`(http, base_url, api_key, model)`。
/// 用 `ModelBase` 消除重复字段定义和 `FromClient` 实现。
#[derive(Clone)]
pub struct ModelBase {
    http: HttpClient,
    base_url: String,
    api_key: String,
    model: String,
}

impl ModelBase {
    pub fn http(&self) -> &HttpClient {
        &self.http
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn to_provider_config(&self) -> crate::types::ProviderConfig {
        crate::types::ProviderConfig {
            api_key: self.api_key.clone(),
            base_url: Some(self.base_url.clone()),
            model: self.model.clone(),
            ..Default::default()
        }
    }
}

impl<Ext: ProviderExt> FromClient<Ext> for ModelBase {
    fn from_client(client: &ProviderClient<Ext>, model: &str) -> Self {
        Self {
            http: client.http.clone(),
            base_url: client.base_url.clone(),
            api_key: client.api_key.clone(),
            model: model.to_string(),
        }
    }
}
