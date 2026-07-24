//! 编译期能力检查验证。

#[cfg(test)]
use crate::impls::deepseek::DeepSeek;
#[cfg(test)]
use crate::impls::ollama::Ollama;
#[cfg(test)]
use crate::impls::openai::OpenAI;
#[cfg(test)]
use crate::impls::zhipu::Zhipu;
#[cfg(test)]
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
