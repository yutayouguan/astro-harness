//! Gateway Event Hooks：扫描 `~/.astro/hooks/<name>/HOOK.yaml` + 进程内 handler。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use tracing::warn;

use crate::outcome::HookPayload;

pub type GatewayHandlerFn = Arc<dyn Fn(&str, &HookPayload) + Send + Sync>;

#[derive(Debug, Clone, Deserialize)]
pub struct HookManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub events: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveredHook {
    pub dir: PathBuf,
    pub manifest: HookManifest,
}

/// Gateway 钩子注册表：清单发现 + 按 name 绑定 Rust handler。
#[derive(Default, Clone)]
pub struct GatewayHookRegistry {
    discovered: Arc<std::sync::Mutex<Vec<DiscoveredHook>>>,
    handlers: Arc<std::sync::Mutex<HashMap<String, GatewayHandlerFn>>>,
}

impl GatewayHookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 扫描 `{root}/hooks/*/HOOK.yaml`。
    pub fn discover(&self, root: &Path) -> anyhow::Result<usize> {
        let hooks_dir = root.join("hooks");
        let mut found = Vec::new();
        if !hooks_dir.is_dir() {
            if let Ok(mut g) = self.discovered.lock() {
                *g = found;
            }
            return Ok(0);
        }
        for entry in fs::read_dir(&hooks_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let yaml = entry.path().join("HOOK.yaml");
            if !yaml.is_file() {
                continue;
            }
            let text = fs::read_to_string(&yaml)?;
            match serde_yaml::from_str::<HookManifest>(&text) {
                Ok(manifest) => found.push(DiscoveredHook {
                    dir: entry.path(),
                    manifest,
                }),
                Err(err) => {
                    warn!(path = %yaml.display(), %err, "invalid HOOK.yaml; skipped");
                }
            }
        }
        let n = found.len();
        if let Ok(mut g) = self.discovered.lock() {
            *g = found;
        }
        Ok(n)
    }

    pub fn register_handler<F>(&self, name: impl Into<String>, f: F)
    where
        F: Fn(&str, &HookPayload) + Send + Sync + 'static,
    {
        if let Ok(mut map) = self.handlers.lock() {
            map.insert(name.into(), Arc::new(f));
        }
    }

    /// 触发某 gateway 事件：对订阅了该事件且已绑定 handler 的清单调用。
    pub fn fire(&self, event: &str, payload: &HookPayload) {
        let discovered = self
            .discovered
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        let handlers = self.handlers.lock().map(|g| g.clone()).unwrap_or_default();
        for d in discovered {
            if !d.manifest.events.iter().any(|e| e == event || e == "*") {
                continue;
            }
            match handlers.get(&d.manifest.name) {
                Some(h) => {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        h(event, payload)
                    }));
                }
                None => {
                    warn!(
                        hook = %d.manifest.name,
                        event,
                        "gateway hook has no registered handler; skipped"
                    );
                }
            }
        }
    }

    pub fn discovered(&self) -> Vec<DiscoveredHook> {
        self.discovered
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// 为尚未绑定自定义 handler 的清单安装默认 tracing 日志 handler，
    /// 使仅放置 `HOOK.yaml` 即可在启动后观察到 Gateway 事件。
    pub fn install_logging_fallbacks(&self) {
        let discovered = self.discovered();
        let existing = self
            .handlers
            .lock()
            .map(|g| g.keys().cloned().collect::<std::collections::HashSet<_>>())
            .unwrap_or_default();
        for d in discovered {
            if existing.contains(&d.manifest.name) {
                continue;
            }
            let name = d.manifest.name.clone();
            self.register_handler(name.clone(), move |event, payload| {
                tracing::info!(
                    gateway_hook = %name,
                    %event,
                    session = %payload.session_id,
                    detail = %payload.detail,
                    "gateway hook"
                );
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn discover_and_fire() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hooks").join("audit");
        fs::create_dir_all(&hook_dir).unwrap();
        fs::write(
            hook_dir.join("HOOK.yaml"),
            "name: audit\nevents:\n  - gateway:startup\n  - agent:end\n",
        )
        .unwrap();
        let reg = GatewayHookRegistry::new();
        assert_eq!(reg.discover(dir.path()).unwrap(), 1);
        let hits = Arc::new(AtomicUsize::new(0));
        let h = Arc::clone(&hits);
        reg.register_handler("audit", move |ev, _| {
            if ev == "gateway:startup" {
                h.fetch_add(1, Ordering::SeqCst);
            }
        });
        reg.fire("gateway:startup", &HookPayload::default());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }
}
