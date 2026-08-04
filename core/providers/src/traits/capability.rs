//! 编译期能力标记系统。
//!
//! ```rust,ignore
//! // 厂商声明能力：
//! impl Capabilities for DeepSeek {
//!     type Chat = Capable<OpenAICompletionModel<Self>>;
//!     type Embedding = Nothing;  // 编译期阻止 embed()
//! }
//! ```

use std::marker::PhantomData;

mod sealed {
    pub trait Sealed {}
}

/// 能力标记 trait（密封，仅 `Capable<M>` 和 `Nothing` 可实现）。
pub trait Capability: sealed::Sealed + Send + Sync + 'static {}

/// 表示"具备此能力"，`M` 为该能力的具体模型实现。
pub struct Capable<M>(PhantomData<M>);

impl<M: Send + Sync + 'static> sealed::Sealed for Capable<M> {}
impl<M: Send + Sync + 'static> Capability for Capable<M> {}

/// 表示"不具备此能力"。
pub struct Nothing;

impl sealed::Sealed for Nothing {}
impl Capability for Nothing {}

/// 厂商能力声明。
///
/// 每个关联类型为 `Capable<ConcreteModel>` 或 `Nothing`。
/// blanket impl 基于这些类型自动为 `Client<Ext>` 实现对应 trait。
pub trait Capabilities: Send + Sync + 'static {
    /// 聊天补全。
    type Chat: Capability;
    /// 向量嵌入。
    type Embedding: Capability;
    /// 图片生成。
    type ImageGen: Capability;
    /// 视频生成。
    type VideoGen: Capability;
    /// 语音合成。
    type TTS: Capability;
    /// 音乐生成。
    type MusicGen: Capability;
    /// 语音识别。
    type ASR: Capability;
}
