//! 文本转语音工具：调用 OpenAI `audio/speech`，将结果写入工作区 `generated/`。
//!
//! 密钥优先取当前聊天 Provider 为 OpenAI 时的 `chat_api_key`，否则回退 `OPENAI_API_KEY`。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `tts` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct TtsArgs {
    /// 待合成的文本（不可为空）。
    pub text: String,
    /// 音色：`alloy` / `echo` / `fable` / `onyx` / `nova` / `shimmer`；缺省 `alloy`。
    #[serde(default)]
    pub voice: Option<String>,
}

/// 向注册表登记 `tts` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "tts".to_string(),
        toolset: "tts".to_string(),
        description: "Convert text to speech via OpenAI audio/speech when OpenAI credentials are available."
            .to_string(),
        schema: schema_for_args::<TtsArgs>(),
        check_fn: None,
        icon: "mic",
    });
}

/// 请求 OpenAI TTS，将 mp3 保存到工作区并返回路径。
///
/// # 错误
/// 无可用 API Key、HTTP 非成功，或写文件失败。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: TtsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("tts 参数无效: {e}"))?;
    let text = parsed.text.trim();
    if text.is_empty() {
        anyhow::bail!("tts 需要 text");
    }
    let voice = parsed.voice.as_deref().unwrap_or("alloy");

    let api_key = if !ctx.chat_api_key.is_empty() && ctx.chat_provider == "openai" {
        ctx.chat_api_key.clone()
    } else {
        std::env::var("OPENAI_API_KEY").unwrap_or_default()
    };
    if api_key.is_empty() {
        anyhow::bail!("tts 需要 OpenAI API Key（当前聊天 Provider 为 OpenAI，或设置 OPENAI_API_KEY）");
    }

    let base = if !ctx.chat_base_url.is_empty() && ctx.chat_provider == "openai" {
        providers::http_stream::openai_compatible_base(&ctx.chat_base_url)
    } else {
        "https://api.openai.com/v1".to_string()
    };
    let url = format!("{base}/audio/speech");
    let body = serde_json::json!({
        "model": "gpt-4o-mini-tts",
        "input": text,
        "voice": voice,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .bearer_auth(&api_key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    if !status.is_success() {
        let err = resp.text().await.unwrap_or_default();
        anyhow::bail!("TTS 失败 HTTP {status}: {err}");
    }
    let bytes = resp.bytes().await?;
    let dir = ctx.workspace_dir.join("generated");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!(
        "tts-{}-{}.mp3",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    ));
    std::fs::write(&path, &bytes)?;
    Ok(format!("语音已生成：{}", path.display()))
}
