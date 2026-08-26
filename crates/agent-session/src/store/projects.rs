//! Project 实体 CRUD（v19）。

use anyhow::{anyhow, Result};
use rusqlite::params;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::SessionStore;

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

    /// 删除项目，返回孤儿会话 ID 列表（原先关联到该项目的会话）。
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
        // 解除会话关联
        tx.execute(
            "UPDATE sessions SET project_id = NULL WHERE project_id = ?1",
            params![id],
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
        let mut stmt = self
            .conn
            .prepare(
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
        if self.get_project(DEFAULT_PROJECT_ID)?.is_none() {
            self.conn.execute(
                "INSERT INTO projects (id, name, position)
                 VALUES (?1, '默认工作空间', -1)",
                params![DEFAULT_PROJECT_ID],
            )?;
            self.conn.execute(
                "INSERT INTO project_roots (project_id, path, root_position)
                 VALUES (?1, ?2, 0)",
                params![DEFAULT_PROJECT_ID, canonical[0]],
            )?;
        } else {
            self.conn.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                params![DEFAULT_PROJECT_ID],
            )?;
            self.conn.execute(
                "INSERT INTO project_roots (project_id, path, root_position)
                 VALUES (?1, ?2, 0)",
                params![DEFAULT_PROJECT_ID, canonical[0]],
            )?;
        }
        self.conn.execute(
            "UPDATE sessions SET project_id = ?1 WHERE project_id IS NULL",
            params![DEFAULT_PROJECT_ID],
        )?;
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
}
