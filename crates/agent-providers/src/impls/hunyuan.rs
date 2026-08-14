//! 腾讯混元 — OpenAI 兼容。
openai_compat!(Hunyuan, "hunyuan", "https://api.hunyuan.cloud.tencent.com/v1",
               stream_usage: false, caps: chat_media);
