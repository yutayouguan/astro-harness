use crate::http_proxy::run_http_proxy_with_listener;
use crate::{NetworkPolicyDecider, NetworkProxyState};
use anyhow::{ensure, Context, Result};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

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
