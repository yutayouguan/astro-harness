//! 媒体生成 Tauri 命令：TTS、图片、视频、音乐的直接调用入口。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TtsResult {
    pub path: String,
    pub mime_type: String,
    pub duration_ms: Option<u64>,
}

/// 合成语音：调用当前活跃供应商的 TTS API，保存音频到 artifacts 并返回路径。
#[tauri::command]
pub async fn tts_synthesize(
    text: String,
    provider_id: Option<String>,
    model: Option<String>,
    voice: Option<String>,
) -> Result<TtsResult, String> {
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            tts_inner(&text, provider_id.as_deref(), model.as_deref(), voice.as_deref()).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn tts_inner(
    text: &str,
    provider_id: Option<&str>,
    model: Option<&str>,
    _voice: Option<&str>,
) -> Result<TtsResult, String> {
    use providers::registry::ProviderRegistry;
    use providers::trait_::ProviderConfig;

    let pid = provider_id.unwrap_or_else(|| {
        let state = crate::providers_commands::get_providers_state();
        if let Ok(s) = &state {
            if let Some(ref id) = s.active_provider_id {
                return match id.as_str() {
                    "google" => "google",
                    "minimax" => "minimax",
                    "openai" => "openai",
                    _ => "openai",
                };
            }
        }
        "openai"
    });

    let auth = providers::AuthKind::for_provider(pid);
    let api_key = if auth == providers::AuthKind::None {
        String::new()
    } else {
        providers::profile::read_env_api_key(pid)
            .ok_or_else(|| format!("未找到 {} 的 API Key 环境变量", pid))?
    };
    let base_url = Some(providers::profile::default_base_for(pid).to_string())
        .filter(|s| !s.is_empty());
    let config = ProviderConfig {
        api_key,
        base_url,
        model: model.unwrap_or("").to_string(),
        temperature: 0.7,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!({}),
        previous_interaction_id: None,
    };

    let registry = ProviderRegistry::new();
    let provider = registry.get(pid)
        .ok_or_else(|| format!("未找到供应商: {}", pid))?;

    let result = provider.text_to_speech(text, &config).await.map_err(|e| e.to_string())?;

    // 保存到 artifacts
    let artifacts_dir = home::default_memory_dir().join("artifacts").join("tts");
    std::fs::create_dir_all(&artifacts_dir).map_err(|e| e.to_string())?;
    let ext = if result.mime_type.contains("wav") { "wav" } else { "mp3" };
    let filename = format!("tts_{}.{}", chrono::Local::now().format("%Y%m%d_%H%M%S_%3f"), ext);
    let path = artifacts_dir.join(&filename);
    std::fs::write(&path, &result.data).map_err(|e| e.to_string())?;

    Ok(TtsResult {
        path: path.to_string_lossy().to_string(),
        mime_type: result.mime_type,
        duration_ms: result.duration_ms,
    })
}

/// 语音识别：将音频字节转写为文本（OpenAI Whisper 兼容 API）。
#[tauri::command]
pub async fn speech_to_text(
    audio_base64: String,
    filename: Option<String>,
    provider_id: Option<String>,
    model: Option<String>,
) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(async {
            stt_inner(&audio_base64, filename.as_deref(), provider_id.as_deref(), model.as_deref()).await
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn stt_inner(
    audio_base64: &str,
    filename: Option<&str>,
    provider_id: Option<&str>,
    model: Option<&str>,
) -> Result<String, String> {
    use providers::trait_::ProviderConfig;

    let audio_bytes = base64_decode(audio_base64).map_err(|e| format!("base64 解码失败: {e}"))?;
    if audio_bytes.is_empty() {
        return Err("音频数据为空".into());
    }

    let pid = provider_id.unwrap_or("openai");
    let auth = providers::AuthKind::for_provider(pid);
    let api_key = if auth == providers::AuthKind::None {
        String::new()
    } else {
        providers::profile::read_env_api_key(pid)
            .ok_or_else(|| format!("未找到 {} 的 API Key 环境变量", pid))?
    };
    let base_url = Some(providers::profile::default_base_for(pid).to_string())
        .filter(|s| !s.is_empty());
    let config = ProviderConfig {
        api_key,
        base_url,
        model: model.unwrap_or("whisper-v3-turbo").to_string(),
        temperature: 0.0,
        max_tokens: 4096,
        thinking_enabled: false,
        reasoning_effort: String::new(),
        additional_params: serde_json::json!({}),
        previous_interaction_id: None,
    };

    let fname = filename.unwrap_or("recording.webm");
    let http = reqwest::Client::new();
    providers::openai::media_compat::openai_audio_transcriptions(
        &http, &audio_bytes, fname, &config,
    )
    .await
    .map_err(|e| e.to_string())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let input = input.trim();
    let input = if let Some(rest) = input.strip_prefix("data:") {
        rest.split_once(',').map(|(_, b)| b).unwrap_or(input)
    } else {
        input
    };
    base64::engine::general_purpose::STANDARD
        .decode(input)
        .map_err(|e| e.to_string())
}
