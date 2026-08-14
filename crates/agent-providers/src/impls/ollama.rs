//! Ollama — 本地 OpenAI 兼容，无需 API Key。
openai_compat!(Ollama, "ollama", "http://localhost:11434/v1",
               stream_usage: true, auth: none, caps: chat_only);
