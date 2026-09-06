use std::path::PathBuf;

use ::session::ConversationStore;
use agent_protocol::ResponseItem;

/// 从 canonical rollout 恢复 Agent 历史；无 rollout 时读取 SQLite 的原生 ResponseItem 索引。
pub async fn hydrate_response_history(
    memory_dir: &std::path::Path,
    sessions: &dyn ConversationStore,
    session_id: &str,
) -> anyhow::Result<Vec<ResponseItem>> {
    let rollout_root = memory_dir.join("sessions").join("rollouts");
    if let Some(path) = agent_rollout::find_rollout(&rollout_root, session_id)? {
        let items = agent_rollout::read_rollout(&path).await?;
        let has_authoritative_history = items.iter().any(|item| {
            matches!(
                item,
                agent_rollout::RolloutItem::ResponseItem(_)
                    | agent_rollout::RolloutItem::Compacted(_)
                    | agent_rollout::RolloutItem::EventMsg(
                        agent_protocol::EventMsg::ThreadRolledBack(_)
                    )
            )
        });
        if has_authoritative_history {
            return Ok(agent_rollout::effective_response_history(&items));
        }
    }
    Ok(sessions
        .get_response_items(session_id)
        .await?
        .into_iter()
        .map(|stored| stored.item)
        .collect())
}

/// 会话级项目根：`ASTRO_SESSION_WORKTREE=1` 且存在 `ASTRO_PROJECT_ROOT`（或 cwd git root）时启用。
pub fn resolve_session_project_root() -> Option<PathBuf> {
    let flag = std::env::var("ASTRO_SESSION_WORKTREE").unwrap_or_default();
    if flag != "1" && !flag.eq_ignore_ascii_case("true") {
        return None;
    }
    crate::git_worktree::resolve_project_root(None)
        .filter(|p| crate::git_worktree::find_git_root(p).is_some() || p.is_dir())
}
