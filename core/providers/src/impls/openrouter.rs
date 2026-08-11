//! OpenRouter — OpenAI 兼容网关。
openai_compat!(OpenRouter, "openrouter", "https://openrouter.ai/api/v1",
               stream_usage: true, caps: chat_only);
