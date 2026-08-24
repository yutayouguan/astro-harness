//! Project 实体 CRUD（v19）。

use anyhow::{anyhow, Result};
use rusqlite::params;

use super::SessionStore;

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

    /// 创建新项目。
    pub fn create_project(&self, name: &str, roots: &[&str]) -> Result<Project> {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let next_pos: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM projects",
            [],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO projects (id, name, position) VALUES (?1, ?2, ?3)",
            params![id, name, next_pos],
        )?;
        for root in roots {
            self.conn.execute(
                "INSERT OR IGNORE INTO project_roots (project_id, path) VALUES (?1, ?2)",
                params![id, *root],
            )?;
        }
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
            tx.execute(
                "DELETE FROM project_roots WHERE project_id = ?1",
                params![id],
            )?;
            for root in roots {
                tx.execute(
                    "INSERT INTO project_roots (project_id, path) VALUES (?1, ?2)",
                    params![id, *root],
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
        let tx = self.conn.unchecked_transaction()?;
        // 收集将要变成孤儿的会话 ID
        let mut stmt = tx.prepare(
            "SELECT id FROM sessions WHERE project_id = ?1",
        )?;
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
        let mut stmt = tx.prepare(
            "SELECT id FROM projects ORDER BY position ASC",
        )?;
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
            "SELECT path FROM project_roots WHERE project_id = ?1 ORDER BY path",
        )?;
        let roots = stmt
            .query_map(params![project_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(roots)
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::SessionStore;

    fn open_memory() -> SessionStore {
        SessionStore::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn create_and_list_projects() {
        let store = open_memory();
        let p1 = store.create_project("My App", &["/home/user/my-app"]).unwrap();
        assert_eq!(p1.name, "My App");
        assert_eq!(p1.roots, vec!["/home/user/my-app"]);
        assert_eq!(p1.position, 0);

        let p2 = store.create_project("Backend", &["/home/user/backend"]).unwrap();
        assert_eq!(p2.position, 1);

        let all = store.list_projects().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "My App");
        assert_eq!(all[1].name, "Backend");
    }

    #[test]
    fn update_project_name_and_roots() {
        let store = open_memory();
        let p = store.create_project("Old Name", &["/a"]).unwrap();
        let updated = store
            .update_project(&p.id, Some("New Name"), None, Some(&["/a", "/b"]))
            .unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.roots.len(), 2);
    }

    #[test]
    fn delete_project_returns_orphans() {
        let store = open_memory();
        let p = store.create_project("Test", &["/test"]).unwrap();
        store.ensure_session("s1", "tauri").unwrap();
        store.assign_session_to_project("s1", &p.id).unwrap();
        let orphans = store.delete_project(&p.id).unwrap();
        assert_eq!(orphans, vec!["s1"]);
        assert!(store.list_projects().unwrap().is_empty());
    }

    #[test]
    fn move_project_reorders() {
        let store = open_memory();
        let a = store.create_project("A", &[]).unwrap();
        let b = store.create_project("B", &[]).unwrap();
        let c = store.create_project("C", &[]).unwrap();
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
        let p = store.create_project("Proj", &["/proj"]).unwrap();
        store.ensure_session("s1", "tauri").unwrap();
        store.assign_session_to_project("s1", &p.id).unwrap();
        store.unassign_session_from_project("s1").unwrap();
        // 不应 panic
    }

    #[test]
    fn assign_if_unassigned_preserves_existing_project() {
        let store = open_memory();
        let first = store.create_project("First", &["/first"]).unwrap();
        let second = store.create_project("Second", &["/second"]).unwrap();
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
        let p = store.create_project("WebApp", &["/code/web"]).unwrap();
        let found = store.find_project_by_root("/code/web").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, p.id);
        let miss = store.find_project_by_root("/nonexistent").unwrap();
        assert!(miss.is_none());
    }
}
