use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::Local;
use serde::{Deserialize, Serialize};

use crate::model::{
    is_reserved_agent_tool_name, is_valid_agent_tool_name, NewWorkflow, NodeType, Workflow,
    WorkflowAgentToolPatch, WorkflowToolExposure,
};

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
        let root = home::workflows_dir(&home::default_memory_dir());
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
        // 自动备份上一版本（保留最近 5 个备份）
        if path.exists() {
            self.rotate_backups(&path);
        }
        let unique = format!("json.tmp.{}", uuid::Uuid::new_v4());
        let tmp = path.with_extension(unique);
        let data = serde_json::to_string_pretty(file)?;
        fs::write(&tmp, &data).context("写入临时文件失败")?;
        fs::rename(&tmp, &path).context("重命名临时文件失败")?;
        Ok(())
    }

    fn rotate_backups(&self, path: &std::path::Path) {
        const MAX_BACKUPS: usize = 5;
        let backup_dir = self.root.join("backups");
        if fs::create_dir_all(&backup_dir).is_err() {
            return;
        }
        let ts = Local::now().format("%Y%m%d_%H%M%S");
        let backup_name = format!("workflows_{}.json", ts);
        let _ = fs::copy(path, backup_dir.join(&backup_name));
        // 清理超出数量的旧备份
        if let Ok(entries) = fs::read_dir(&backup_dir) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with("workflows_"))
                .collect();
            files.sort_by_key(|e| e.file_name());
            while files.len() > MAX_BACKUPS {
                if let Some(old) = files.first() {
                    let _ = fs::remove_file(old.path());
                }
                files.remove(0);
            }
        }
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
        if wf.description.len() > 4096 {
            anyhow::bail!("工作流描述过长，上限 4096 字节");
        }
        if wf.agent_tool.exposure != WorkflowToolExposure::Disabled {
            let tool_name = wf.agent_tool_name();
            if !is_valid_agent_tool_name(&tool_name) {
                anyhow::bail!(
                    "Agent 工具名 `{tool_name}` 无效：只允许 1-64 位字母、数字、下划线或连字符"
                );
            }
            if is_reserved_agent_tool_name(&tool_name) {
                anyhow::bail!("Agent 工具名 `{tool_name}` 为 workflow namespace 保留名");
            }
            if wf
                .agent_tool
                .input_schema
                .get("type")
                .and_then(|v| v.as_str())
                != Some("object")
            {
                anyhow::bail!("Agent 工具 input_schema 必须是 type=object 的 JSON Schema");
            }
            jsonschema::meta::validate(&wf.agent_tool.input_schema)
                .map_err(|error| anyhow::anyhow!("Agent 工具 input_schema 无效: {error}"))?;
            anyhow::ensure!(
                serde_json::to_vec(&wf.agent_tool.input_schema)?.len() <= 64 * 1024,
                "Agent 工具 input_schema 过大，上限 64 KiB"
            );
            anyhow::ensure!(
                wf.agent_tool.output_description.len() <= 2048,
                "Agent 工具输出说明过长，上限 2048 字节"
            );
            anyhow::ensure!(
                wf.agent_tool.examples.len() <= 16
                    && serde_json::to_vec(&wf.agent_tool.examples)?.len() <= 32 * 1024,
                "Agent 工具调用示例过多或过大"
            );
            if wf.enabled
                && wf
                    .nodes
                    .iter()
                    .any(|node| !node.disabled && node.node_type == NodeType::Code)
            {
                anyhow::bail!(
                    "含 Code 节点的工作流尚未接入 Agent 沙箱；请先将 Agent 工具暴露设为 disabled"
                );
            }
        }
        Ok(())
    }

    fn validate_unique_agent_tool_name(file: &WorkflowsFile, wf: &Workflow) -> Result<()> {
        if wf.agent_tool.exposure == WorkflowToolExposure::Disabled {
            return Ok(());
        }
        let name = wf.agent_tool_name();
        if let Some(existing) = file.workflows.iter().find(|candidate| {
            candidate.id != wf.id
                && candidate.agent_tool.exposure != WorkflowToolExposure::Disabled
                && candidate.agent_tool_name() == name
        }) {
            anyhow::bail!("Agent 工具名 `{name}` 已被工作流「{}」使用", existing.name);
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
        let _guard = STORE_LOCK
            .lock()
            .map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
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
        let _guard = STORE_LOCK
            .lock()
            .map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
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
        let _guard = STORE_LOCK
            .lock()
            .map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        if let Some(position) = file.workflows.iter().position(|workflow| workflow.id == id) {
            let mut updated = file.workflows[position].clone();
            updated.enabled = enabled;
            if enabled {
                Self::validate_workflow(&updated)?;
                Self::validate_unique_agent_tool_name(&file, &updated)?;
            }
            updated.updated_at = Local::now().to_rfc3339();
            file.workflows[position] = updated;
            self.save(&file)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn update_agent_tool(&self, id: &str, patch: WorkflowAgentToolPatch) -> Result<bool> {
        let _guard = STORE_LOCK
            .lock()
            .map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        let Some(position) = file.workflows.iter().position(|workflow| workflow.id == id) else {
            return Ok(false);
        };
        let mut updated = file.workflows[position].clone();
        updated.agent_tool.apply_patch(patch);
        Self::validate_workflow(&updated)?;
        Self::validate_unique_agent_tool_name(&file, &updated)?;
        updated.updated_at = Local::now().to_rfc3339();
        file.workflows[position] = updated;
        self.save(&file)?;
        Ok(true)
    }

    /// Upsert：id 已存在则更新，否则插入。加锁防并发写入丢数据。
    pub fn save_workflow(&self, wf: Workflow) -> Result<Workflow> {
        Self::validate_workflow(&wf)?;
        let _guard = STORE_LOCK
            .lock()
            .map_err(|e| anyhow::anyhow!("store lock: {e}"))?;
        let mut file = self.load()?;
        let mut wf = wf;
        wf.updated_at = Local::now().to_rfc3339();
        Self::validate_unique_agent_tool_name(&file, &wf)?;
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

    #[test]
    fn rejects_duplicate_agent_tool_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(dir.path()).unwrap();
        let mut first = store
            .create(NewWorkflow {
                name: "first".into(),
                description: String::new(),
            })
            .unwrap();
        first.agent_tool.name = "shared_name".into();
        store.save_workflow(first).unwrap();

        let mut second = store
            .create(NewWorkflow {
                name: "second".into(),
                description: String::new(),
            })
            .unwrap();
        second.agent_tool.name = "shared_name".into();
        let error = store.save_workflow(second).unwrap_err();
        assert!(error.to_string().contains("已被工作流"));
    }

    #[test]
    fn missing_agent_tool_defaults_to_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(dir.path()).unwrap();
        let raw = serde_json::json!({
            "workflows": [{
                "id": "legacy",
                "name": "legacy",
                "description": "",
                "enabled": true,
                "nodes": [],
                "edges": [],
                "variables": {},
                "created_at": "now",
                "updated_at": "now"
            }]
        });
        std::fs::write(
            dir.path().join("workflows.json"),
            serde_json::to_vec(&raw).unwrap(),
        )
        .unwrap();

        let workflows = store.list().unwrap();
        assert_eq!(
            workflows[0].agent_tool.exposure,
            WorkflowToolExposure::Disabled
        );
    }

    #[test]
    fn agent_tool_patch_preserves_unmentioned_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = WorkflowStore::open(dir.path()).unwrap();
        let mut workflow = store
            .create(NewWorkflow {
                name: "patch".into(),
                description: String::new(),
            })
            .unwrap();
        workflow.agent_tool.name = "stable_name".into();
        store.save_workflow(workflow.clone()).unwrap();

        store
            .update_agent_tool(
                &workflow.id,
                WorkflowAgentToolPatch {
                    confirmation: Some(crate::model::WorkflowToolConfirmation::Always),
                    ..WorkflowAgentToolPatch::default()
                },
            )
            .unwrap();
        let updated = store.get(&workflow.id).unwrap().unwrap();
        assert_eq!(updated.agent_tool.name, "stable_name");
        assert_eq!(
            updated.agent_tool.confirmation,
            crate::model::WorkflowToolConfirmation::Always
        );
    }

    #[test]
    fn enabled_agent_tool_rejects_unsandboxed_code_nodes() {
        let mut workflow = Workflow::new(NewWorkflow {
            name: "code".into(),
            description: String::new(),
        });
        workflow.enabled = true;
        workflow.nodes.push(crate::model::WorkflowNode {
            id: "code".into(),
            node_type: NodeType::Code,
            label: "Code".into(),
            position: crate::model::Position { x: 0.0, y: 0.0 },
            config: serde_json::json!({"language": "bash", "source": "echo unsafe"}),
            disabled: false,
        });

        let error = WorkflowStore::validate_workflow(&workflow)
            .expect_err("Agent-callable Code workflow must fail closed");
        assert!(error.to_string().contains("尚未接入 Agent 沙箱"));

        workflow.agent_tool.exposure = WorkflowToolExposure::Disabled;
        WorkflowStore::validate_workflow(&workflow)
            .expect("non-Agent workflow may keep using the existing Code executor");
    }
}
