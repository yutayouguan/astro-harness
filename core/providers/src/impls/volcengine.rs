//! 火山引擎 (Doubao) — OpenAI 兼容。
openai_compat!(Volcengine, "volcengine", "https://ark.cn-beijing.volces.com/api/v3",
               stream_usage: false, caps: chat_media);
