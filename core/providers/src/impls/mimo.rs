//! 小米 Mimo — OpenAI 兼容。
openai_compat!(Mimo, "mimo", "https://api.xiaomimimo.com/v1",
               stream_usage: true, caps: chat_only);
