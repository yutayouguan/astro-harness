//! Project 实体 CRUD（v19）。

use agent_db::sqlx::{self, Row};
use anyhow::{anyhow, Result};

use super::SessionStore;

/// 内置主空间项目的稳定 ID。
pub const DEFAULT_PROJECT_ID: &str = "default";

/// 从库中读出的 Project 实体。
#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub roots: Vec<String>,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionStore {
    /// 确保内置主空间项目存在，并固定绑定到规范工作区根目录。
    ///
    /// 旧版桌面端曾在空库中创建随机 ID、无 root 的 `Default` 项目；这里会原地迁移
    /// 该记录及其会话关联，避免升级后出现两个默认项目。
    pub async fn ensure_default_project(
        &self,
        name: &str,
        icon: &str,
        root: &str,
    ) -> Result<Project> {
        if let Some(project) = self.get_project(DEFAULT_PROJECT_ID).await? {
            if project.roots.len() == 1 && project.roots[0] == root {
                return Ok(project);
            }
        }

        let mut tx = self.pool.begin().await?;
        // 先执行一次写入来取得 SQLite writer lock。`INSERT OR IGNORE` 既让并发调用
        // 串行化，也避免两个连接都在只读快照中判断项目不存在后再竞争插入。
        let inserted = sqlx::query(
            "INSERT OR IGNORE INTO projects (id, name, icon, position)
             VALUES (?1, ?2, ?3, 0)",
        )
        .bind(DEFAULT_PROJECT_ID)
        .bind(name)
        .bind(icon)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;

        if inserted {
            let legacy = sqlx::query(
                "SELECT p.id, p.position
                 FROM projects p
                 WHERE p.id != ?1
                   AND p.name = 'Default'
                   AND NOT EXISTS (
                       SELECT 1 FROM project_roots r WHERE r.project_id = p.id
                   )
                 ORDER BY p.position ASC
                 LIMIT 1",
            )
            .bind(DEFAULT_PROJECT_ID)
            .fetch_optional(&mut *tx)
            .await?;

            if let Some(row) = legacy {
                let legacy_id: String = row.get(0);
                let legacy_position: i64 = row.get(1);
                sqlx::query("UPDATE projects SET position = ?1 WHERE id = ?2")
                    .bind(legacy_position)
                    .bind(DEFAULT_PROJECT_ID)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("UPDATE sessions SET project_id = ?1 WHERE project_id = ?2")
                    .bind(DEFAULT_PROJECT_ID)
                    .bind(&legacy_id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query("DELETE FROM projects WHERE id = ?1")
                    .bind(&legacy_id)
                    .execute(&mut *tx)
                    .await?;
            } else {
                sqlx::query("UPDATE projects SET position = position + 1 WHERE id != ?1")
                    .bind(DEFAULT_PROJECT_ID)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        let current_roots =
            sqlx::query("SELECT path FROM project_roots WHERE project_id = ?1 ORDER BY path")
                .bind(DEFAULT_PROJECT_ID)
                .fetch_all(&mut *tx)
                .await?;
        let root_is_current =
            current_roots.len() == 1 && current_roots[0].get::<String, _>(0) == root;
        if !root_is_current {
            sqlx::query("DELETE FROM project_roots WHERE project_id = ?1")
                .bind(DEFAULT_PROJECT_ID)
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO project_roots (project_id, path) VALUES (?1, ?2)")
                .bind(DEFAULT_PROJECT_ID)
                .bind(root)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE projects SET updated_at = datetime('now') WHERE id = ?1")
                .bind(DEFAULT_PROJECT_ID)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        self.get_project(DEFAULT_PROJECT_ID)
            .await?
            .ok_or_else(|| anyhow!("default project not found after ensure"))
    }

    /// 按 position 升序列出所有项目（含 roots）。
    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT id, name, icon, position, created_at, updated_at
             FROM projects ORDER BY position ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut projects = Vec::with_capacity(rows.len());
        for r in rows {
            let id: String = r.get(0);
            let roots = self.load_project_roots(&id).await?;
            projects.push(Project {
                id,
                name: r.get(1),
                icon: r.get(2),
                roots,
                position: r.get(3),
                created_at: r.get(4),
                updated_at: r.get(5),
            });
        }
        Ok(projects)
    }

    /// 按 id 读取单个项目。
    pub async fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, name, icon, position, created_at, updated_at
             FROM projects WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(r) => {
                let pid: String = r.get(0);
                let roots = self.load_project_roots(&pid).await?;
                Ok(Some(Project {
                    id: pid,
                    name: r.get(1),
                    icon: r.get(2),
                    roots,
                    position: r.get(3),
                    created_at: r.get(4),
                    updated_at: r.get(5),
                }))
            }
            None => Ok(None),
        }
    }

    /// 按 root 路径查找项目（精确匹配）。
    pub async fn find_project_by_root(&self, path: &str) -> Result<Option<Project>> {
        let row = sqlx::query("SELECT project_id FROM project_roots WHERE path = ?1 LIMIT 1")
            .bind(path)
            .fetch_optional(&self.pool)
            .await?;
        match row {
            Some(r) => self.get_project(&r.get::<String, _>(0)).await,
            None => Ok(None),
        }
    }

    /// 创建新项目。
    pub async fn create_project(&self, name: &str, roots: &[&str]) -> Result<Project> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let next_pos: i64 = sqlx::query("SELECT COALESCE(MAX(position), -1) + 1 FROM projects")
            .fetch_one(&self.pool)
            .await?
            .get(0);
        sqlx::query("INSERT INTO projects (id, name, position) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(name)
            .bind(next_pos)
            .execute(&self.pool)
            .await?;
        for root in roots {
            sqlx::query("INSERT OR IGNORE INTO project_roots (project_id, path) VALUES (?1, ?2)")
                .bind(&id)
                .bind(*root)
                .execute(&self.pool)
                .await?;
        }
        self.get_project(&id)
            .await?
            .ok_or_else(|| anyhow!("project just created but not found"))
    }

    /// 更新项目名称、图标和/或 roots。
    pub async fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        icon: Option<Option<&str>>,
        roots: Option<&[&str]>,
    ) -> Result<Project> {
        if id == DEFAULT_PROJECT_ID && roots.is_some() {
            anyhow::bail!("update_project: default project root is immutable");
        }
        if self.get_project(id).await?.is_none() {
            anyhow::bail!("update_project: project not found");
        }
        let mut tx = self.pool.begin().await?;
        if let Some(name) = name {
            sqlx::query(
                "UPDATE projects SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
            )
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        if let Some(icon_val) = icon {
            sqlx::query(
                "UPDATE projects SET icon = ?1, updated_at = datetime('now') WHERE id = ?2",
            )
            .bind(icon_val)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        if let Some(roots) = roots {
            sqlx::query("DELETE FROM project_roots WHERE project_id = ?1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            for root in roots {
                sqlx::query("INSERT INTO project_roots (project_id, path) VALUES (?1, ?2)")
                    .bind(id)
                    .bind(*root)
                    .execute(&mut *tx)
                    .await?;
            }
            if name.is_none() {
                sqlx::query("UPDATE projects SET updated_at = datetime('now') WHERE id = ?1")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        tx.commit().await?;
        self.get_project(id)
            .await?
            .ok_or_else(|| anyhow!("project not found after update"))
    }

    /// 删除项目，返回孤儿会话 ID 列表（原先关联到该项目的会话）。
    pub async fn delete_project(&self, id: &str) -> Result<Vec<String>> {
        if id == DEFAULT_PROJECT_ID {
            anyhow::bail!("delete_project: default project cannot be deleted");
        }
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query("SELECT id FROM sessions WHERE project_id = ?1")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
        let orphans: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
        sqlx::query("UPDATE sessions SET project_id = NULL WHERE project_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        // project_roots 由 ON DELETE CASCADE 自动清理
        let result = sqlx::query("DELETE FROM projects WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("delete_project: project not found");
        }
        tx.commit().await?;
        Ok(orphans)
    }

    /// 移动项目到 `before_id` 之前；`before_id` 为 `None` 时移到末尾。
    pub async fn move_project(&self, id: &str, before_id: Option<&str>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query("SELECT id FROM projects ORDER BY position ASC")
            .fetch_all(&mut *tx)
            .await?;
        let mut ids: Vec<String> = rows.iter().map(|r| r.get(0)).collect();

        // 移除目标
        let orig_pos = ids.iter().position(|x| x == id);
        if orig_pos.is_none() {
            anyhow::bail!("move_project: project not found");
        }
        ids.retain(|x| x != id);

        // 插入到 before_id 之前或末尾
        match before_id {
            Some(before) => {
                let insert_pos = ids
                    .iter()
                    .position(|x| x == before)
                    .ok_or_else(|| anyhow!("move_project: before_project not found"))?;
                ids.insert(insert_pos, id.to_string());
            }
            None => ids.push(id.to_string()),
        }

        // 重写 position
        for (pos, pid) in ids.iter().enumerate() {
            sqlx::query("UPDATE projects SET position = ?1 WHERE id = ?2")
                .bind(pos as i64)
                .bind(pid)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// 将会话关联到项目。
    pub async fn assign_session_to_project(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<()> {
        let result = sqlx::query("UPDATE sessions SET project_id = ?1 WHERE id = ?2")
            .bind(project_id)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            anyhow::bail!("assign_session_to_project: session not found");
        }
        Ok(())
    }

    /// 解除会话的项目关联。
    pub async fn unassign_session_from_project(&self, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE sessions SET project_id = NULL WHERE id = ?1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---- 内部方法 ----

    async fn load_project_roots(&self, project_id: &str) -> Result<Vec<String>> {
        let rows =
            sqlx::query("SELECT path FROM project_roots WHERE project_id = ?1 ORDER BY path")
                .bind(project_id)
                .fetch_all(&self.pool)
                .await?;
        let roots = rows.iter().map(|r| r.get(0)).collect();
        Ok(roots)
    }
}

#[cfg(test)]
mod tests {
    use agent_db::sqlx::{self, Row};

    use super::DEFAULT_PROJECT_ID;
    use crate::store::SessionStore;

    async fn test_store() -> (tempfile::TempDir, SessionStore) {
        let dir = tempfile::TempDir::new().unwrap();
        let store = SessionStore::open(&dir.path().join("state.db"))
            .await
            .unwrap();
        (dir, store)
    }

    #[tokio::test]
    async fn create_and_list_projects() {
        let (_dir, store) = test_store().await;
        let p1 = store
            .create_project("My App", &["/home/user/my-app"])
            .await
            .unwrap();
        assert_eq!(p1.name, "My App");
        assert_eq!(p1.roots, vec!["/home/user/my-app"]);
        assert_eq!(p1.position, 0);

        let p2 = store
            .create_project("Backend", &["/home/user/backend"])
            .await
            .unwrap();
        assert_eq!(p2.position, 1);

        let all = store.list_projects().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "My App");
        assert_eq!(all[1].name, "Backend");
    }

    #[tokio::test]
    async fn ensure_default_project_is_stable_rooted_and_first() {
        let (_dir, store) = test_store().await;
        store
            .create_project("Existing", &["/existing"])
            .await
            .unwrap();

        let default = store
            .ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace")
            .await
            .unwrap();
        assert_eq!(default.id, DEFAULT_PROJECT_ID);
        assert_eq!(default.name, "主空间");
        assert_eq!(default.icon.as_deref(), Some("astro-space"));
        assert_eq!(default.roots, vec!["/home/user/.astro/workspace"]);
        assert_eq!(default.position, 0);

        sqlx::query("INSERT INTO project_roots (project_id, path) VALUES (?1, '/stale')")
            .bind(DEFAULT_PROJECT_ID)
            .execute(&store.pool)
            .await
            .unwrap();
        let ensured_again = store
            .ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace")
            .await
            .unwrap();
        assert_eq!(ensured_again.id, DEFAULT_PROJECT_ID);
        assert_eq!(ensured_again.roots, vec!["/home/user/.astro/workspace"]);
        let projects = store.list_projects().await.unwrap();
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].id, DEFAULT_PROJECT_ID);
    }

    #[tokio::test]
    async fn concurrent_stores_ensure_only_one_default_project() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("state.db");
        let store_a = SessionStore::open(&path).await.unwrap();
        let store_b = SessionStore::open(&path).await.unwrap();

        let (result_a, result_b) = tokio::join!(
            store_a.ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace"),
            store_b.ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace"),
        );
        assert_eq!(result_a.unwrap().id, DEFAULT_PROJECT_ID);
        assert_eq!(result_b.unwrap().id, DEFAULT_PROJECT_ID);

        let projects = store_a.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, DEFAULT_PROJECT_ID);
    }

    #[tokio::test]
    async fn ensure_default_project_migrates_legacy_empty_project() {
        let (_dir, store) = test_store().await;
        let legacy = store.create_project("Default", &[]).await.unwrap();
        store.ensure_session("s1", "tauri").await.unwrap();
        store
            .assign_session_to_project("s1", &legacy.id)
            .await
            .unwrap();

        let default = store
            .ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace")
            .await
            .unwrap();
        assert_eq!(default.id, DEFAULT_PROJECT_ID);
        assert_eq!(default.roots, vec!["/home/user/.astro/workspace"]);
        assert!(store.get_project(&legacy.id).await.unwrap().is_none());

        let row = sqlx::query("SELECT project_id FROM sessions WHERE id = 's1'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>(0), DEFAULT_PROJECT_ID);
    }

    #[tokio::test]
    async fn default_project_cannot_change_root_or_be_deleted() {
        let (_dir, store) = test_store().await;
        store
            .ensure_default_project("主空间", "astro-space", "/home/user/.astro/workspace")
            .await
            .unwrap();

        let update_error = store
            .update_project(DEFAULT_PROJECT_ID, None, None, Some(&["/other"]))
            .await
            .unwrap_err();
        assert!(update_error.to_string().contains("root is immutable"));

        let delete_error = store.delete_project(DEFAULT_PROJECT_ID).await.unwrap_err();
        assert!(delete_error.to_string().contains("cannot be deleted"));
    }

    #[tokio::test]
    async fn update_project_name_and_roots() {
        let (_dir, store) = test_store().await;
        let p = store.create_project("Old Name", &["/a"]).await.unwrap();
        let updated = store
            .update_project(&p.id, Some("New Name"), None, Some(&["/a", "/b"]))
            .await
            .unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.roots.len(), 2);
    }

    #[tokio::test]
    async fn delete_project_returns_orphans() {
        let (_dir, store) = test_store().await;
        let p = store.create_project("Test", &["/test"]).await.unwrap();
        store.ensure_session("s1", "tauri").await.unwrap();
        store.assign_session_to_project("s1", &p.id).await.unwrap();
        let orphans = store.delete_project(&p.id).await.unwrap();
        assert_eq!(orphans, vec!["s1"]);
        assert!(store.list_projects().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn move_project_reorders() {
        let (_dir, store) = test_store().await;
        let a = store.create_project("A", &[]).await.unwrap();
        let _b = store.create_project("B", &[]).await.unwrap();
        let c = store.create_project("C", &[]).await.unwrap();
        // 把 C 移到 A 前面 → C A B
        store.move_project(&c.id, Some(&a.id)).await.unwrap();
        let all = store.list_projects().await.unwrap();
        assert_eq!(all[0].name, "C");
        assert_eq!(all[1].name, "A");
        assert_eq!(all[2].name, "B");
    }

    #[tokio::test]
    async fn assign_and_unassign_session() {
        let (_dir, store) = test_store().await;
        let p = store.create_project("Proj", &["/proj"]).await.unwrap();
        store.ensure_session("s1", "tauri").await.unwrap();
        store.assign_session_to_project("s1", &p.id).await.unwrap();
        store.unassign_session_from_project("s1").await.unwrap();
        // 不应 panic
    }

    #[tokio::test]
    async fn find_project_by_root() {
        let (_dir, store) = test_store().await;
        let p = store
            .create_project("WebApp", &["/code/web"])
            .await
            .unwrap();
        let found = store.find_project_by_root("/code/web").await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, p.id);
        let miss = store.find_project_by_root("/nonexistent").await.unwrap();
        assert!(miss.is_none());
    }
}
