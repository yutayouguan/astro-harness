//! 从已记录的安装来源重新安装 / 更新本机 Skill。

use anyhow::{bail, Result};

use crate::install::{install_from_ref, InstallOriginHint};
use crate::models::{SkillOriginRecord, SkillUpdateItemResult};
use crate::origins::{find_origin, load_origins};

/// 规范化 Agent id：空/`default` → `workspace`（与 `install` / `origins` 一致）。
fn normalize_agent_id(agent_id: Option<&str>) -> String {
    match agent_id.map(str::trim).filter(|s| !s.is_empty()) {
        Some("default") | None => "workspace".to_string(),
        Some(id) => id.to_string(),
    }
}

fn origin_hint(record: &SkillOriginRecord) -> InstallOriginHint {
    InstallOriginHint {
        name: Some(record.name.clone()),
        store: Some(record.store.clone()),
        folder: Some(record.folder.clone()),
    }
}

/// 按 folder 查找来源并重新安装；无 origin 时返回「无法追溯」错误。
pub async fn update_installed_skill(
    agent_id: Option<&str>,
    folder: &str,
) -> Result<String> {
    let record = find_origin(agent_id, folder)?;
    let Some(record) = record else {
        bail!("无法追溯安装源: {folder}");
    };

    install_from_ref(
        &record.install_ref,
        agent_id,
        Some(origin_hint(&record)),
    )
    .await
}

/// 串行更新当前 Agent 下所有有来源记录的技能；单条失败写入结果 Vec，不中断。
pub async fn update_all_with_origin(
    agent_id: Option<&str>,
) -> Result<Vec<SkillUpdateItemResult>> {
    let target = normalize_agent_id(agent_id);
    let file = load_origins()?;
    let folders: Vec<String> = file
        .records
        .iter()
        .filter(|r| normalize_agent_id(r.agent_id.as_deref()) == target)
        .map(|r| r.folder.clone())
        .collect();

    let mut results = Vec::with_capacity(folders.len());
    for folder in folders {
        let item = match update_installed_skill(agent_id, &folder).await {
            Ok(message) => SkillUpdateItemResult {
                folder,
                ok: true,
                message,
            },
            Err(e) => SkillUpdateItemResult {
                folder,
                ok: false,
                message: e.to_string(),
            },
        };
        results.push(item);
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::tempdir;

    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[tokio::test]
    async fn update_without_origin_errors() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let err = update_installed_skill(Some("workspace"), "missing")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("无法追溯"));
    }

    #[tokio::test]
    async fn update_all_empty_ok() {
        let _guard = ENV_TEST_LOCK.lock().unwrap();
        let dir = tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());

        let r = update_all_with_origin(Some("workspace")).await.unwrap();
        assert!(r.is_empty());
    }
}
