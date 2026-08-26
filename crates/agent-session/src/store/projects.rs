//! Project 实体 CRUD（v19）。

use anyhow::{anyhow, Result};
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::SessionStore;

pub const DEFAULT_PROJECT_ID: &str = "default";
pub const DEFAULT_PROJECT_NAME: &str = "主空间";
/// 前端 `public/project-icons/astro-space.svg` 的品牌文件夹图标。
pub const DEFAULT_PROJECT_ICON: &str = "astro-space";
/// v19 早期版本写入的默认项目名；仅当用户没有自定义过名字时才改写成新名。
const LEGACY_DEFAULT_PROJECT_NAMES: [&str; 1] = ["默认工作空间"];

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
    /// 按 position 升序列出所有项目（含 roots）。
    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, icon, position, created_at, updated_at
             FROM projects ORDER BY position ASC",
        )?;
        let rows: Vec<(String, String, Option<String>, i64, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

        let mut projects = Vec::with_capacity(rows.len());
        for (id, name, icon, position, created_at, updated_at) in rows {
            let roots = self.load_project_roots(&id)?;
            projects.push(Project {
                id,
                name,
                icon,
                roots,
                position,
                created_at,
                updated_at,
            });
        }
        Ok(projects)
    }

    /// 按 id 读取单个项目。
    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let row: Option<(String, String, Option<String>, i64, String, String)> = self
            .conn
            .query_row(
                "SELECT id, name, icon, position, created_at, updated_at
                 FROM projects WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()?;
        match row {
            Some((id, name, icon, position, created_at, updated_at)) => {
                let roots = self.load_project_roots(&id)?;
                Ok(Some(Project {
                    id,
                    name,
                    icon,
                    roots,
                    position,
                    created_at,
                    updated_at,
                }))
            }
            None => Ok(None),
        }
    }

    /// 按 root 路径查找项目（精确匹配）。
    pub fn find_project_by_root(&self, path: &str) -> Result<Option<Project>> {
        let project_id: Option<String> = self
            .conn
            .query_row(
                "SELECT project_id FROM project_roots WHERE path = ?1 LIMIT 1",
                params![path],
                |row| row.get(0),
            )
            .optional()?;
        match project_id {
            Some(pid) => self.get_project(&pid),
            None => Ok(None),
        }
    }

    /// 返回会话当前绑定的项目。
    pub fn project_for_session(&self, session_id: &str) -> Result<Option<Project>> {
        let project_id: Option<String> = self
            .conn
            .query_row(
                "SELECT project_id FROM sessions WHERE id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        match project_id {
            Some(id) => self.get_project(&id),
            None => Ok(None),
        }
    }

    /// 创建新项目。
    pub fn create_project(&self, name: &str, roots: &[&str]) -> Result<Project> {
        let roots = normalize_project_roots(roots)?;
        for root in &roots {
            if self.find_project_by_root(root)?.is_some() {
                anyhow::bail!("project folder already belongs to another project: {root}");
            }
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let tx = self.conn.unchecked_transaction()?;
        let next_pos: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM projects",
            [],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO projects (id, name, position) VALUES (?1, ?2, ?3)",
            params![id, name, next_pos],
        )?;
        for (root_position, root) in roots.iter().enumerate() {
            tx.execute(
                "INSERT INTO project_roots (project_id, path, root_position)
                 VALUES (?1, ?2, ?3)",
                params![id, root, root_position as i64],
            )?;
        }
        tx.commit()?;
        self.get_project(&id)?
            .ok_or_else(|| anyhow!("project just created but not found"))
    }

    /// 更新项目名称、图标和/或 roots。
    pub fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        icon: Option<Option<&str>>,
        roots: Option<&[&str]>,
    ) -> Result<Project> {
        if self.get_project(id)?.is_none() {
            anyhow::bail!("update_project: project not found");
        }
        if id == DEFAULT_PROJECT_ID && roots.is_some() {
            anyhow::bail!("the default project's workspace folder cannot be changed");
        }
        let tx = self.conn.unchecked_transaction()?;
        if let Some(name) = name {
            tx.execute(
                "UPDATE projects SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![name, id],
            )?;
        }
        if let Some(icon_val) = icon {
            tx.execute(
                "UPDATE projects SET icon = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![icon_val, id],
            )?;
        }
        if let Some(roots) = roots {
            let roots = normalize_project_roots(roots)?;
            for root in &roots {
                if self
                    .find_project_by_root(root)?
                    .is_some_and(|project| project.id != id)
                {
                    anyhow::bail!("project folder already belongs to another project: {root}");
                }
            }
            tx.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                params![id],
            )?;
            for (root_position, root) in roots.iter().enumerate() {
                tx.execute(
                    "INSERT INTO project_roots (project_id, path, root_position)
                     VALUES (?1, ?2, ?3)",
                    params![id, root, root_position as i64],
                )?;
            }
            if name.is_none() {
                tx.execute(
                    "UPDATE projects SET updated_at = datetime('now') WHERE id = ?1",
                    params![id],
                )?;
            }
        }
        tx.commit()?;
        self.get_project(id)?
            .ok_or_else(|| anyhow!("project not found after update"))
    }

    /// 删除项目，返回受影响会话 ID；默认项目存在时将这些会话迁回 default。
    pub fn delete_project(&self, id: &str) -> Result<Vec<String>> {
        if id == DEFAULT_PROJECT_ID {
            anyhow::bail!("the default project cannot be deleted");
        }
        let tx = self.conn.unchecked_transaction()?;
        // 收集将要变成孤儿的会话 ID
        let mut stmt = tx.prepare("SELECT id FROM sessions WHERE project_id = ?1")?;
        let orphans: Vec<String> = stmt
            .query_map(params![id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        // 默认项目存在时迁回 default；测试/迁移场景尚未创建 default 时才回退 NULL。
        tx.execute(
            "UPDATE sessions
             SET project_id = CASE
                 WHEN EXISTS(SELECT 1 FROM projects WHERE id = ?1) THEN ?1
                 ELSE NULL
             END
             WHERE project_id = ?2",
            params![DEFAULT_PROJECT_ID, id],
        )?;
        // project_roots 由 ON DELETE CASCADE 自动清理
        let changed = tx.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        if changed == 0 {
            anyhow::bail!("delete_project: project not found");
        }
        tx.commit()?;
        Ok(orphans)
    }

    /// 移动项目到 `before_id` 之前；`before_id` 为 `None` 时移到末尾。
    pub fn move_project(&self, id: &str, before_id: Option<&str>) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        // 读取全部 project id 按 position 排序
        let mut stmt = tx.prepare("SELECT id FROM projects ORDER BY position ASC")?;
        let mut ids: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);

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
            tx.execute(
                "UPDATE projects SET position = ?1 WHERE id = ?2",
                params![pos as i64, pid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 将会话关联到项目。
    pub fn assign_session_to_project(&self, session_id: &str, project_id: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE sessions SET project_id = ?1 WHERE id = ?2",
            params![project_id, session_id],
        )?;
        if changed == 0 {
            anyhow::bail!("assign_session_to_project: session not found");
        }
        Ok(())
    }

    /// 仅在会话尚未归属项目时建立关联；已有归属保持不变。
    pub fn assign_session_to_project_if_unassigned(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE sessions SET project_id = ?1
             WHERE id = ?2 AND project_id IS NULL",
            params![project_id, session_id],
        )?;
        if changed == 0 {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE id = ?1)",
                params![session_id],
                |row| row.get(0),
            )?;
            if !exists {
                anyhow::bail!("assign_session_to_project_if_unassigned: session not found");
            }
        }
        Ok(())
    }

    /// 解除会话的项目关联。
    pub fn unassign_session_from_project(&self, session_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET project_id = NULL WHERE id = ?1",
            params![session_id],
        )?;
        Ok(())
    }

    // ---- internal ----

    fn load_project_roots(&self, project_id: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT path FROM project_roots
                 WHERE project_id = ?1
                 ORDER BY root_position ASC, path ASC",
        )?;
        let roots = stmt
            .query_map(params![project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(roots)
    }

    /// 确保稳定默认项目存在，并把未归属会话归入默认项目。
    pub fn ensure_default_project(&self, workspace_root: &Path) -> Result<Project> {
        let workspace = workspace_root.to_string_lossy().into_owned();
        let canonical = normalize_project_roots(&[workspace.as_str()])?;
        let root = canonical[0].as_str();
        let tx = self.conn.unchecked_transaction()?;
        // foreign_keys 未开启时 project_roots 会留下孤儿行，使 find_project_by_root
        // 误判工作区目录空闲，从而放行重复的默认项目。
        tx.execute(
            "DELETE FROM project_roots
             WHERE project_id NOT IN (SELECT id FROM projects)",
            [],
        )?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1)",
            params![DEFAULT_PROJECT_ID],
            |row| row.get(0),
        )?;
        if exists {
            for legacy in LEGACY_DEFAULT_PROJECT_NAMES {
                tx.execute(
                    "UPDATE projects SET name = ?1, updated_at = datetime('now')
                     WHERE id = ?2 AND name = ?3",
                    params![DEFAULT_PROJECT_NAME, DEFAULT_PROJECT_ID, legacy],
                )?;
            }
            // 没有图标的历史默认项目补上品牌图标；用户已选的图标保持不变。
            tx.execute(
                "UPDATE projects SET icon = ?1, updated_at = datetime('now')
                 WHERE id = ?2 AND (icon IS NULL OR TRIM(icon) = '')",
                params![DEFAULT_PROJECT_ICON, DEFAULT_PROJECT_ID],
            )?;
        } else {
            tx.execute(
                "INSERT INTO projects (id, name, icon, position) VALUES (?1, ?2, ?3, -1)",
                params![
                    DEFAULT_PROJECT_ID,
                    DEFAULT_PROJECT_NAME,
                    DEFAULT_PROJECT_ICON
                ],
            )?;
        }
        tx.execute(
            "DELETE FROM project_roots WHERE project_id = ?1",
            params![DEFAULT_PROJECT_ID],
        )?;
        tx.execute(
            "INSERT INTO project_roots (project_id, path, root_position)
             VALUES (?1, ?2, 0)",
            params![DEFAULT_PROJECT_ID, root],
        )?;
        // 旧版本用随机 id 建过默认项目，只以工作区为唯一目录的项目一律并入 default。
        let mut stmt = tx.prepare(
            "SELECT pr.project_id FROM project_roots pr
             WHERE pr.path = ?1
               AND pr.project_id != ?2
               AND (SELECT COUNT(*) FROM project_roots x
                    WHERE x.project_id = pr.project_id) = 1",
        )?;
        let duplicates: Vec<String> = stmt
            .query_map(params![root, DEFAULT_PROJECT_ID], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for duplicate in &duplicates {
            tx.execute(
                "UPDATE sessions SET project_id = ?1 WHERE project_id = ?2",
                params![DEFAULT_PROJECT_ID, duplicate],
            )?;
            tx.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                params![duplicate],
            )?;
            tx.execute("DELETE FROM projects WHERE id = ?1", params![duplicate])?;
        }
        tx.execute(
            "UPDATE sessions SET project_id = ?1 WHERE project_id IS NULL",
            params![DEFAULT_PROJECT_ID],
        )?;
        tx.commit()?;
        if !duplicates.is_empty() {
            tracing::info!(
                count = duplicates.len(),
                "merged legacy default projects into the stable default project"
            );
        }
        self.get_project(DEFAULT_PROJECT_ID)?
            .ok_or_else(|| anyhow!("default project not found after ensure"))
    }
}

use rusqlite::OptionalExtension;

fn normalize_project_roots(roots: &[&str]) -> Result<Vec<String>> {
    if roots.is_empty() {
        anyhow::bail!("a project must contain at least one folder");
    }
    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(roots.len());
    for raw in roots {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            anyhow::bail!("project folder cannot be empty");
        }
        let path = PathBuf::from(trimmed);
        if !path.is_absolute() {
            anyhow::bail!("project folder must be an absolute path: {trimmed}");
        }
        if !path.is_dir() {
            anyhow::bail!("project folder does not exist or is not a directory: {trimmed}");
        }
        let canonical = std::fs::canonicalize(&path)?;
        let value = canonical.to_string_lossy().into_owned();
        if seen.insert(value.clone()) {
            normalized.push(value);
        }
    }
    if normalized.is_empty() {
        anyhow::bail!("a project must contain at least one unique folder");
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use crate::store::SessionStore;

    fn open_memory() -> SessionStore {
        SessionStore::open(std::path::Path::new(":memory:")).unwrap()
    }

    fn make_root(base: &tempfile::TempDir, name: &str) -> String {
        let path = base.path().join(name);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn create_and_list_projects() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let app = make_root(&base, "my-app");
        let backend = make_root(&base, "backend");
        let p1 = store.create_project("My App", &[&app]).unwrap();
        assert_eq!(p1.name, "My App");
        assert_eq!(p1.roots, vec![app]);
        assert_eq!(p1.position, 0);

        let p2 = store.create_project("Backend", &[&backend]).unwrap();
        assert_eq!(p2.position, 1);

        let all = store.list_projects().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "My App");
        assert_eq!(all[1].name, "Backend");
    }

    #[test]
    fn update_project_name_and_roots() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let a = make_root(&base, "a");
        let b = make_root(&base, "b");
        let p = store.create_project("Old Name", &[&a]).unwrap();
        let updated = store
            .update_project(&p.id, Some("New Name"), None, Some(&[&b, &a]))
            .unwrap();
        assert_eq!(updated.roots, vec![b, a]);
    }

    #[test]
    fn delete_project_returns_orphans() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let root = make_root(&base, "test");
        let p = store.create_project("Test", &[&root]).unwrap();
        store.ensure_session("s1", "tauri").unwrap();
        store.assign_session_to_project("s1", &p.id).unwrap();
        let orphans = store.delete_project(&p.id).unwrap();
        assert_eq!(orphans, vec!["s1"]);
        assert!(store.list_projects().unwrap().is_empty());
    }

    #[test]
    fn move_project_reorders() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let ra = make_root(&base, "a");
        let rb = make_root(&base, "b");
        let rc = make_root(&base, "c");
        let a = store.create_project("A", &[&ra]).unwrap();
        let _b = store.create_project("B", &[&rb]).unwrap();
        let c = store.create_project("C", &[&rc]).unwrap();
        // 把 C 移到 A 前面 → C A B
        store.move_project(&c.id, Some(&a.id)).unwrap();
        let all = store.list_projects().unwrap();
        assert_eq!(all[0].name, "C");
        assert_eq!(all[1].name, "A");
        assert_eq!(all[2].name, "B");
    }

    #[test]
    fn assign_and_unassign_session() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let root = make_root(&base, "proj");
        let p = store.create_project("Proj", &[&root]).unwrap();
        store.ensure_session("s1", "tauri").unwrap();
        store.assign_session_to_project("s1", &p.id).unwrap();
        store.unassign_session_from_project("s1").unwrap();
        // 不应 panic
    }

    #[test]
    fn assign_if_unassigned_preserves_existing_project() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let first_root = make_root(&base, "first");
        let second_root = make_root(&base, "second");
        let first = store.create_project("First", &[&first_root]).unwrap();
        let second = store.create_project("Second", &[&second_root]).unwrap();
        store.ensure_session("s1", "tauri").unwrap();

        store
            .assign_session_to_project_if_unassigned("s1", &first.id)
            .unwrap();
        store
            .assign_session_to_project_if_unassigned("s1", &second.id)
            .unwrap();

        let first_sessions = store
            .list_sessions_by_project(crate::store::SessionListFilter::Active, 10, &first.id)
            .unwrap();
        let second_sessions = store
            .list_sessions_by_project(crate::store::SessionListFilter::Active, 10, &second.id)
            .unwrap();
        assert_eq!(first_sessions.len(), 1);
        assert!(second_sessions.is_empty());
    }

    #[test]
    fn find_project_by_root() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let root = make_root(&base, "web");
        let p = store.create_project("WebApp", &[&root]).unwrap();
        let found = store.find_project_by_root(&root).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, p.id);
        let miss = store.find_project_by_root("/nonexistent").unwrap();
        assert!(miss.is_none());
    }

    #[test]
    fn ensure_default_project_backfills_unassigned_sessions() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = base.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        store.ensure_session("orphan", "tauri").unwrap();

        let project = store.ensure_default_project(&workspace).unwrap();
        assert_eq!(project.id, crate::store::projects::DEFAULT_PROJECT_ID);
        assert_eq!(
            store.project_for_session("orphan").unwrap().unwrap().id,
            project.id
        );
        assert!(store.delete_project(&project.id).is_err());
    }

    #[test]
    fn ensure_default_project_merges_legacy_default_projects() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = make_root(&base, "workspace");
        let legacy = store.create_project("默认工作空间", &[&workspace]).unwrap();
        store.ensure_session("legacy-session", "tauri").unwrap();
        store
            .assign_session_to_project("legacy-session", &legacy.id)
            .unwrap();

        let default = store
            .ensure_default_project(std::path::Path::new(&workspace))
            .unwrap();

        assert_eq!(default.id, crate::store::projects::DEFAULT_PROJECT_ID);
        assert_eq!(store.list_projects().unwrap().len(), 1);
        assert_eq!(
            store
                .project_for_session("legacy-session")
                .unwrap()
                .unwrap()
                .id,
            default.id
        );
    }

    #[test]
    fn ensure_default_project_seeds_brand_icon_without_touching_custom_choice() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = make_root(&base, "workspace");
        let path = std::path::Path::new(&workspace);

        let created = store.ensure_default_project(path).unwrap();
        assert_eq!(
            created.icon.as_deref(),
            Some(crate::store::projects::DEFAULT_PROJECT_ICON)
        );

        store
            .update_project(&created.id, None, Some(Some("folder-rust")), None)
            .unwrap();
        let kept = store.ensure_default_project(path).unwrap();
        assert_eq!(kept.icon.as_deref(), Some("folder-rust"));

        store
            .update_project(&created.id, None, Some(None), None)
            .unwrap();
        let reseeded = store.ensure_default_project(path).unwrap();
        assert_eq!(
            reseeded.icon.as_deref(),
            Some(crate::store::projects::DEFAULT_PROJECT_ICON)
        );
    }

    #[test]
    fn ensure_default_project_renames_legacy_name_but_keeps_custom_one() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = make_root(&base, "workspace");
        let path = std::path::Path::new(&workspace);

        let created = store.ensure_default_project(path).unwrap();
        assert_eq!(created.name, crate::store::projects::DEFAULT_PROJECT_NAME);

        store
            .update_project(&created.id, Some("默认工作空间"), None, None)
            .unwrap();
        let renamed = store.ensure_default_project(path).unwrap();
        assert_eq!(renamed.name, crate::store::projects::DEFAULT_PROJECT_NAME);

        store
            .update_project(&created.id, Some("我的空间"), None, None)
            .unwrap();
        let kept = store.ensure_default_project(path).unwrap();
        assert_eq!(kept.name, "我的空间");
    }

    #[test]
    fn ensure_default_project_ignores_multi_root_projects_containing_workspace() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = make_root(&base, "workspace");
        let other = make_root(&base, "other");
        let combined = store
            .create_project("Combined", &[&other, &workspace])
            .unwrap();

        store
            .ensure_default_project(std::path::Path::new(&workspace))
            .unwrap();

        assert!(store.get_project(&combined.id).unwrap().is_some());
    }

    #[test]
    fn deleting_project_moves_sessions_back_to_default() {
        let store = open_memory();
        let base = tempfile::tempdir().unwrap();
        let workspace = make_root(&base, "workspace");
        let custom_root = make_root(&base, "custom");
        let default = store
            .ensure_default_project(std::path::Path::new(&workspace))
            .unwrap();
        let custom = store.create_project("Custom", &[&custom_root]).unwrap();
        store.ensure_session("session", "tauri").unwrap();
        store
            .assign_session_to_project("session", &custom.id)
            .unwrap();

        store.delete_project(&custom.id).unwrap();
        assert_eq!(
            store.project_for_session("session").unwrap().unwrap().id,
            default.id
        );
    }
}
