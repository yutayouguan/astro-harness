//! 编译期能力检查验证。

#[cfg(test)]
mod tests {
    use crate::impls::deepseek::DeepSeek;
    use crate::impls::ollama::Ollama;
    use crate::impls::openai::OpenAI;
    use crate::impls::zhipu::Zhipu;
    use crate::traits::client::{ChatClient, ProviderClient};

    #[test]
    fn deepseek_has_chat() {
        let client = ProviderClient::new("test-key", DeepSeek);
        let _model = client.completion_model("deepseek-chat");
    }

    #[test]
    fn openai_has_chat() {
        let client = ProviderClient::new("test-key", OpenAI);
        let _model = client.completion_model("gpt-4o");
    }

    #[test]
    fn ollama_has_chat_no_key() {
        let client = ProviderClient::new("", Ollama);
        let _model = client.completion_model("llama3.3");
    }

    #[test]
    fn zhipu_has_chat() {
        let client = ProviderClient::new("test-key", Zhipu);
        let _model = client.completion_model("glm-4");
    }

    // 以下代码如果取消注释应该编译失败（能力检查）：
    // #[test]
    // fn deepseek_no_embedding() {
    //     let client = ProviderClient::new("key", DeepSeek);
    //     let _model = client.embedding_model("embed"); // 编译错误！
    // }
}
