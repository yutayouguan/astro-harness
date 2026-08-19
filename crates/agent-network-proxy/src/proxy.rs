use crate::http_proxy::run_http_proxy_with_listener;
use crate::{BlockedRequest, NetworkPolicyDecider, NetworkProxyState};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

const PROXY_URL_ENV_KEYS: &[&str] = &[
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "http_proxy",
    "https_proxy",
    "ALL_PROXY",
    "all_proxy",
];
const NO_PROXY_ENV_KEYS: &[&str] = &["NO_PROXY", "no_proxy"];
pub const PROXY_ACTIVE_ENV_KEY: &str = "ASTRO_NETWORK_PROXY_ACTIVE";
pub const DEFAULT_NO_PROXY_VALUE: &str = concat!(
    "localhost,127.0.0.1,::1,",
    "10.0.0.0/8,",
    "172.16.0.0/12,",
    "192.168.0.0/16"
);

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedNetworkSandboxContext {
    #[serde(default)]
    pub loopback_ports: Vec<u16>,
    #[serde(default)]
    pub allow_local_binding: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedManagedNetwork {
    pub env: HashMap<String, String>,
    pub sandbox_context: ManagedNetworkSandboxContext,
}

#[derive(Clone, Default)]
pub struct NetworkProxyBuilder {
    state: Option<Arc<NetworkProxyState>>,
    http_addr: Option<SocketAddr>,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
}

impl NetworkProxyBuilder {
    pub fn state(mut self, state: Arc<NetworkProxyState>) -> Self {
        self.state = Some(state);
        self
    }

    pub fn http_addr(mut self, addr: SocketAddr) -> Self {
        self.http_addr = Some(addr);
        self
    }

    pub fn policy_decider<D>(mut self, decider: D) -> Self
    where
        D: NetworkPolicyDecider,
    {
        self.policy_decider = Some(Arc::new(decider));
        self
    }

    pub fn policy_decider_arc(mut self, decider: Arc<dyn NetworkPolicyDecider>) -> Self {
        self.policy_decider = Some(decider);
        self
    }

    pub async fn build(self) -> Result<NetworkProxy> {
        let state = self
            .state
            .context("NetworkProxyBuilder requires a state; supply one via builder.state(...)")?;
        let requested_addr = self
            .http_addr
            .unwrap_or_else(|| SocketAddr::from((Ipv4Addr::LOCALHOST, 0)));
        ensure!(
            requested_addr.ip().is_loopback(),
            "network proxy must bind to a loopback address"
        );
        let listener = TcpListener::bind(requested_addr)
            .await
            .with_context(|| format!("bind HTTP proxy listener on {requested_addr}"))?;
        let http_addr = listener
            .local_addr()
            .context("read HTTP proxy listener address")?;
        Ok(NetworkProxy {
            state,
            http_addr,
            listener: Arc::new(Mutex::new(Some(listener))),
            policy_decider: self.policy_decider,
        })
    }
}

#[derive(Clone)]
pub struct NetworkProxy {
    state: Arc<NetworkProxyState>,
    http_addr: SocketAddr,
    listener: Arc<Mutex<Option<TcpListener>>>,
    policy_decider: Option<Arc<dyn NetworkPolicyDecider>>,
}

impl std::fmt::Debug for NetworkProxy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NetworkProxy")
            .field("http_addr", &self.http_addr)
            .finish_non_exhaustive()
    }
}

impl NetworkProxy {
    pub fn builder() -> NetworkProxyBuilder {
        NetworkProxyBuilder::default()
    }

    pub fn http_addr(&self) -> SocketAddr {
        self.http_addr
    }

    pub fn prepare(&self, mut env: HashMap<String, String>) -> PreparedManagedNetwork {
        let proxy_url = format!("http://{}", self.http_addr);
        for key in PROXY_URL_ENV_KEYS {
            env.insert((*key).to_string(), proxy_url.clone());
        }
        let no_proxy = if self.state.allow_local_binding() {
            DEFAULT_NO_PROXY_VALUE
        } else {
            ""
        };
        for key in NO_PROXY_ENV_KEYS {
            env.insert((*key).to_string(), no_proxy.to_string());
        }
        env.insert(PROXY_ACTIVE_ENV_KEY.to_string(), "1".to_string());
        PreparedManagedNetwork {
            env,
            sandbox_context: ManagedNetworkSandboxContext {
                loopback_ports: vec![self.http_addr.port()],
                allow_local_binding: self.state.allow_local_binding(),
            },
        }
    }

    pub fn take_blocked_requests(&self) -> Vec<BlockedRequest> {
        self.state.take_blocked_requests()
    }

    pub async fn run(&self) -> Result<NetworkProxyHandle> {
        let listener = self
            .listener
            .lock()
            .await
            .take()
            .context("network proxy listener is already running")?;
        let state = Arc::clone(&self.state);
        let policy_decider = self.policy_decider.clone();
        let task = tokio::spawn(async move {
            run_http_proxy_with_listener(state, listener, policy_decider).await
        });
        Ok(NetworkProxyHandle {
            task: Some(task),
            completed: false,
        })
    }
}

pub struct StartedNetworkProxy {
    proxy: NetworkProxy,
    _handle: NetworkProxyHandle,
}

impl StartedNetworkProxy {
    pub async fn start(state: Arc<NetworkProxyState>) -> Result<Self> {
        let proxy = NetworkProxy::builder().state(state).build().await?;
        let handle = proxy.run().await?;
        Ok(Self {
            proxy,
            _handle: handle,
        })
    }

    pub fn proxy(&self) -> &NetworkProxy {
        &self.proxy
    }
}

pub struct NetworkProxyHandle {
    task: Option<JoinHandle<Result<()>>>,
    completed: bool,
}

impl NetworkProxyHandle {
    pub async fn wait(mut self) -> Result<()> {
        let task = self.task.as_mut().context("missing network proxy task")?;
        task.await.context("network proxy task failed")??;
        self.task.take();
        self.completed = true;
        Ok(())
    }

    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(task) = self.task.take() {
            task.abort();
            let _ = task.await;
        }
        self.completed = true;
        Ok(())
    }
}

impl Drop for NetworkProxyHandle {
    fn drop(&mut self) {
        if !self.completed {
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }
    }
}
