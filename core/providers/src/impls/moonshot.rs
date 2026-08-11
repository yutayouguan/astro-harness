//! 月之暗面 Kimi — OpenAI 兼容。
openai_compat!(Moonshot, "moonshot", "https://api.moonshot.cn/v1",
               stream_usage: true, caps: chat_only);
