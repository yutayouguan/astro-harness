//! NVIDIA — OpenAI 兼容。
openai_compat!(Nvidia, "nvidia", "https://integrate.api.nvidia.com/v1",
               stream_usage: true, caps: chat_only);
