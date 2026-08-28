//! MCP Streamable HTTP OAuth 2.1 + PKCE 与系统凭证库集成。
//!
//! 配置文件只保存认证方式；client registration 与 token 始终进入系统 Keychain。

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use http::{HeaderName, HeaderValue};
use rmcp::model::ClientJsonRpcMessage;
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, CredentialStore, OAuthState, StoredCredentials,
};
use rmcp::transport::streamable_http_client::{
    SseError, StreamableHttpClient, StreamableHttpError, StreamableHttpPostResponse,
};
use sse_stream::Sse;

use crate::config::{McpHttpAuth, McpServerConfig, McpTransportType};

const KEYRING_SERVICE: &str = "com.astro-agent.mcp.oauth";

/// OAuth 流程开始后的浏览器 URL 与内存状态机。
pub struct McpOAuthSession {
    state: OAuthState,
    authorization_url: String,
}

impl McpOAuthSession {
    pub fn authorization_url(&self) -> &str {
        &self.authorization_url
    }

    /// 校验 state/issuer、交换 code，并由 rmcp 将 token 保存到 Keychain store。
    pub async fn complete(mut self, callback_url: &str) -> Result<(), AuthError> {
        self.state.handle_callback_url(callback_url).await
    }
}

/// 对同一种 transport 泛型统一匿名与 OAuth 客户端，避免公开 Server 被强制登录。
#[derive(Clone)]
pub(crate) enum OptionalAuthClient {
    Plain(reqwest::Client),
    OAuth(AuthClient<reqwest::Client>),
}

impl StreamableHttpClient for OptionalAuthClient {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        match self {
            Self::Plain(client) => {
                client
                    .post_message(uri, message, session_id, auth_header, custom_headers)
                    .await
            }
            Self::OAuth(client) => {
                client
                    .post_message(uri, message, session_id, auth_header, custom_headers)
                    .await
            }
        }
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        match self {
            Self::Plain(client) => {
                client
                    .delete_session(uri, session_id, auth_header, custom_headers)
                    .await
            }
            Self::OAuth(client) => {
                client
                    .delete_session(uri, session_id, auth_header, custom_headers)
                    .await
            }
        }
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        last_event_id: Option<String>,
        auth_header: Option<String>,
        custom_headers: HashMap<HeaderName, HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        match self {
            Self::Plain(client) => {
                client
                    .get_stream(uri, session_id, last_event_id, auth_header, custom_headers)
                    .await
            }
            Self::OAuth(client) => {
                client
                    .get_stream(uri, session_id, last_event_id, auth_header, custom_headers)
                    .await
            }
        }
    }
}

#[derive(Clone)]
struct McpKeyringCredentialStore {
    account: String,
}

impl McpKeyringCredentialStore {
    fn new(config: &McpServerConfig) -> Self {
        Self {
            account: oauth_keyring_account(config),
        }
    }
}

#[async_trait]
impl CredentialStore for McpKeyringCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(KEYRING_SERVICE, &account).map_err(auth_store_error)?;
            match entry.get_password() {
                Ok(encoded) => serde_json::from_str(&encoded).map(Some).map_err(|error| {
                    AuthError::InternalError(format!("invalid OAuth credential: {error}"))
                }),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(error) => Err(auth_store_error(error)),
            }
        })
        .await
        .map_err(|error| {
            AuthError::InternalError(format!("OAuth credential task failed: {error}"))
        })?
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let account = self.account.clone();
        let encoded = serde_json::to_string(&credentials).map_err(|error| {
            AuthError::InternalError(format!("encode OAuth credential: {error}"))
        })?;
        tokio::task::spawn_blocking(move || {
            keyring::Entry::new(KEYRING_SERVICE, &account)
                .map_err(auth_store_error)?
                .set_password(&encoded)
                .map_err(auth_store_error)
        })
        .await
        .map_err(|error| {
            AuthError::InternalError(format!("OAuth credential task failed: {error}"))
        })?
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let account = self.account.clone();
        tokio::task::spawn_blocking(move || {
            let entry = keyring::Entry::new(KEYRING_SERVICE, &account).map_err(auth_store_error)?;
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(error) => Err(auth_store_error(error)),
            }
        })
        .await
        .map_err(|error| {
            AuthError::InternalError(format!("OAuth credential task failed: {error}"))
        })?
    }
}

fn auth_store_error(error: keyring::Error) -> AuthError {
    AuthError::InternalError(format!("OAuth secure storage unavailable: {error}"))
}

fn oauth_keyring_account(config: &McpServerConfig) -> String {
    let mut server_hasher = std::collections::hash_map::DefaultHasher::new();
    config.id.hash(&mut server_hasher);
    let mut url_hasher = std::collections::hash_map::DefaultHasher::new();
    config.url.hash(&mut url_hasher);
    format!(
        "{:016x}.{:016x}",
        server_hasher.finish(),
        url_hasher.finish()
    )
}

/// HTTP 且未指定第一方会话认证时可使用标准 OAuth。
pub fn is_oauth_available(config: &McpServerConfig) -> bool {
    config.r#type == McpTransportType::StreamableHttp && config.auth != Some(McpHttpAuth::Chatgpt)
}

/// 创建 OAuth 浏览器授权流程。PKCE、state 与 metadata discovery 由 rmcp 实现。
pub async fn begin_oauth(
    config: &McpServerConfig,
    redirect_uri: &str,
) -> Result<McpOAuthSession, AuthError> {
    if !is_oauth_available(config) {
        return Err(AuthError::AuthorizationFailed(
            "this MCP server does not support Astro OAuth login".into(),
        ));
    }
    let client = oauth_http_client()?;
    let mut state = OAuthState::new(&config.url, Some(client)).await?;
    if let OAuthState::Unauthorized(manager) = &mut state {
        manager.set_credential_store(McpKeyringCredentialStore::new(config));
    }
    state
        .start_authorization(&[], redirect_uri, Some("Astro Agent"))
        .await?;
    let authorization_url = state.get_authorization_url().await?;
    Ok(McpOAuthSession {
        state,
        authorization_url,
    })
}

/// 若 Keychain 中已有 token，返回自动刷新并回写凭证的 OAuth HTTP client。
pub(crate) async fn http_client_for(
    config: &McpServerConfig,
    allow_oauth: bool,
) -> Result<(OptionalAuthClient, bool), AuthError> {
    let client = oauth_http_client()?;
    if !allow_oauth || !is_oauth_available(config) {
        return Ok((OptionalAuthClient::Plain(client), false));
    }
    let mut manager = AuthorizationManager::new(&config.url).await?;
    manager.with_client(client.clone())?;
    manager.set_credential_store(McpKeyringCredentialStore::new(config));
    if manager.initialize_from_store().await? {
        Ok((
            OptionalAuthClient::OAuth(AuthClient::new(client, manager)),
            true,
        ))
    } else {
        Ok((OptionalAuthClient::Plain(client), false))
    }
}

/// 清除指定 Server 的 registration/token；配置本身不变。
pub async fn clear_oauth_credentials(config: &McpServerConfig) -> Result<(), AuthError> {
    McpKeyringCredentialStore::new(config).clear().await
}

fn oauth_http_client() -> Result<reqwest::Client, AuthError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| AuthError::InternalError(format!("build OAuth HTTP client: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_config() -> McpServerConfig {
        McpServerConfig {
            id: "docs server".into(),
            name: "Docs".into(),
            description: String::new(),
            r#type: McpTransportType::StreamableHttp,
            command: String::new(),
            args: Vec::new(),
            env: HashMap::new(),
            env_vars: Vec::new(),
            url: "https://mcp.example.test/mcp".into(),
            headers: HashMap::new(),
            bearer_token_env_var: None,
            env_http_headers: HashMap::new(),
            auth: None,
            enabled: true,
            required: false,
            cwd: None,
            startup_timeout_secs: None,
            tool_timeout_secs: None,
            enabled_tools: None,
            disabled_tools: Vec::new(),
            default_tools_approval_mode: types::McpToolApprovalMode::Auto,
            tools: HashMap::new(),
            discovered: Vec::new(),
        }
    }

    #[test]
    fn keyring_account_is_stable_and_does_not_contain_url() {
        let config = http_config();
        let account = oauth_keyring_account(&config);
        assert_eq!(account, oauth_keyring_account(&config));
        assert_eq!(account.split('.').count(), 2);
        assert!(!account.contains("docs server"));
        assert!(!account.contains("example.test"));
    }

    #[test]
    fn chatgpt_auth_is_not_exposed_as_standard_oauth() {
        let mut config = http_config();
        assert!(is_oauth_available(&config));
        config.auth = Some(McpHttpAuth::Chatgpt);
        assert!(!is_oauth_available(&config));
    }
}
