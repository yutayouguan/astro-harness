use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::model::{NewWorkflow, Workflow};

static STORE_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WorkflowsFile {
    workflows: Vec<Workflow>,
}

pub struct WorkflowStore {
    root: PathBuf,
}

impl WorkflowStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).context("创建 workflows 目录失败")?;
        Ok(Self { root })
    }

    pub fn open_default() -> Result<Self> {
        let root = home::default_memory_dir().join("workflows");
        Self::open(root)
    }

    fn file_path(&self) -> PathBuf {
        self.root.join("workflows.json")
    }

    fn load(&self) -> Result<WorkflowsFile> {
        let path = self.file_path();
        if !path.exists() {
            return Ok(WorkflowsFile::default());
        }
        let data = fs::read_to_string(&path).context("读取 workflows.json 失败")?;
        serde_json::from_str(&data).context("解析 workflows.json 失败")
    }

    fn save(&self, file: &WorkflowsFile) -> Result<()> {
        let path = self.file_path();
        let unique = format!("json.tmp.{}", uuid::Uuid::new_v4());
        let tmp = path.with_extension(unique);
        let data = serde_json::to_string_pretty(file)?;
        fs::write(&tmp, &data).context("写入临时文件失败")?;
        fs::rename(&tmp, &path).context("重命名临时文件失败")?;
        Ok(())
    }

    /// 校验工作流结构合法性
    pub fn validate_workflow(wf: &Workflow) -> Result<()> {
        if wf.nodes.len() > 500 {
            anyhow::bail!("节点数量过多（{}），上限 500", wf.nodes.len());
        }
        if wf.edges.len() > 2000 {
            anyhow::bail!("连线数量过多（{}），上限 2000", wf.edges.len());
        }
        if wf.name.len() > 200 {
            anyhow::bail!("工作流名称过长（{} 字节），上限 200", wf.name.len());
        }
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Workflow>> {
        Ok(self.load()?.workflows)
    }

    pub fn get(&self, id: &str) -> Result<Option<Workflow>> {
        let file = self.load()?;
        Ok(file.workflows.into_iter().find(|w| w.id == id))
    }

    pub fn create(&self, input: NewWorkflow) -> Result<Workflow> {
        let mut file = self.load()?;
        let wf = Workflow::new(input);
        file.workflows.push(wf.clone());
        self.save(&file)?;
        Ok(wf)
    }

    pub fn update(&self, updated: Workflow) -> Result<Option<Workflow>> {
        let _guard = STORE_LOCK.lock().map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        if let Some(pos) = file.workflows.iter().position(|w| w.id == updated.id) {
            let mut wf = updated;
            wf.updated_at = Local::now().to_rfc3339();
            file.workflows[pos] = wf.clone();
            self.save(&file)?;
            Ok(Some(wf))
        } else {
            Ok(None)
        }
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let _guard = STORE_LOCK.lock().map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        let before = file.workflows.len();
        file.workflows.retain(|w| w.id != id);
        if file.workflows.len() < before {
            self.save(&file)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        let _guard = STORE_LOCK.lock().map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        if let Some(wf) = file.workflows.iter_mut().find(|w| w.id == id) {
            wf.enabled = enabled;
            wf.updated_at = Local::now().to_rfc3339();
            self.save(&file)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn set_ai_callable(&self, id: &str, callable: bool) -> Result<bool> {
        let _guard = STORE_LOCK.lock().map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        if let Some(wf) = file.workflows.iter_mut().find(|w| w.id == id) {
            wf.ai_callable = callable;
            wf.updated_at = Local::now().to_rfc3339();
            self.save(&file)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Upsert：id 已存在则更新，否则插入。加锁防并发写入丢数据。
    pub fn save_workflow(&self, wf: Workflow) -> Result<Workflow> {
        Self::validate_workflow(&wf)?;
        let _guard = STORE_LOCK.lock().map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        let mut wf = wf;
        wf.updated_at = Local::now().to_rfc3339();
        if let Some(pos) = file.workflows.iter().position(|w| w.id == wf.id) {
            file.workflows[pos] = wf.clone();
        } else {
            wf.created_at = wf.updated_at.clone();
            file.workflows.push(wf.clone());
        }
        self.save(&file)?;
        Ok(wf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(dir.path()).unwrap();

        assert!(store.list().unwrap().is_empty());

        let wf = store
            .create(NewWorkflow {
                name: "测试".into(),
                description: "desc".into(),
            })
            .unwrap();
        assert_eq!(store.list().unwrap().len(), 1);

        let got = store.get(&wf.id).unwrap().unwrap();
        assert_eq!(got.name, "测试");

        store.set_enabled(&wf.id, true).unwrap();
        let got = store.get(&wf.id).unwrap().unwrap();
        assert!(got.enabled);

        store.delete(&wf.id).unwrap();
        assert!(store.list().unwrap().is_empty());
    }
}
