# Agent 市场

> 阶段：系统设计 | 状态：定稿 | 说明：.agent 包格式、安装流程、精选目录

## 概述

Agent 市场允许用户分享、发现、一键安装完整的工作区配置包（`.agent` 文件）。与插件市场不同，Agent 市场分发的是**完整的工作区配置**，包含 Skill 集合、Prompt 模板和工作区参数，用户安装后即可获得一个完整的垂直方向助手。

---

## 1. 与插件市场的区别

| 对比项 | 插件市场 | Agent 市场 |
| ---- | ---- | ---- |
| 分发单位 | 单个 Skill / MCP Server | 完整工作区配置包 |
| 安装结果 | 增加一个 Skill/工具 | 创建一个新工作区 |
| 包含内容 | 一个功能实现 | Skill 集合 + Prompt + 配置 |
| 典型使用场景 | "我需要一个搜索工具" | "我需要一个代码审查助手" |

---

## 2. `.agent` 包格式

`.agent` 文件本质是 ZIP 压缩包，内部目录结构如下：

```
my-code-assistant.agent (ZIP)
├── manifest.json          # 必须：包元数据
├── skills/                # 可选：内嵌 Skill 定义
│   ├── code-review/
│   │   └── SKILL.md
│   └── test-generator/
│       └── SKILL.md
├── prompts/               # 可选：Prompt 模板
│   ├── system.md          # 系统提示词
│   └── templates/
│       └── review.md
├── config.toml            # 可选：工作区配置（不含 API Key）
└── README.md              # 可选：使用说明
```

### manifest.json 示例

```json
{
  "$schema": "https://releases.astro-agent.io/schemas/agent-manifest-v1.json",
  "name": "code-assistant",
  "version": "1.2.0",
  "description": "全功能代码审查与测试生成助手",
  "author": {
    "name": "username",
    "email": "user@example.com",
    "url": "https://github.com/username"
  },
  "license": "MIT",
  "icon": "icon.png",
  "homepage": "https://github.com/username/code-assistant",
  "compatibility": {
    "min_agent_version": "0.5.0"
  },
  "permissions": ["ReadFile", "WriteFile", "ExecuteBash", "NetworkRead"],
  "plugins": ["arxiv-researcher"],
  "dependencies": {
    "skills": {
      "web-search": ">=1.0.0",
      "shell-executor": ">=2.0.0"
    },
    "mcp": {
      "filesystem-mcp": ">=0.5.0"
    }
  },
  "workspace": {
    "default_model": "claude-sonnet-4-5",
    "mode": "code",
    "features": ["knowledge-base", "eval"]
  }
}
```

### config.toml 示例

```toml
[agent]
# 不包含任何密钥，仅包含行为配置
max_iterations = 20
allow_shell = true
allow_file_write = true
context_window_usage_threshold = 0.75

[skills]
enabled = ["code-review", "test-generator", "web-search"]

[prompts]
system = "prompts/system.md"
```

> **权限映射**：`.agent` 包 `config.toml` 中的权限字段映射到统一的 `PermissionSet` 模型：
>
> - `allow_shell = true` → `Permission::ExecuteBash`
> - `allow_file_write = true` → `Permission::WriteFile`
> - `allow_network = true` → `Permission::NetworkRead` + `Permission::NetworkWrite`
>
> 安装 `.agent` 包时，系统将这些权限声明转换为 `PermissionSet`，并在审批弹窗中展示。用户可在安装时拒绝部分权限。详见 `06-安全边界.md` 和术语统一规范。

---

## 3. AgentPackage Rust 实现

```rust
// crates/agent-core/src/marketplace/package.rs

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use zip::{ZipArchive, ZipWriter};

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct Manifest {
    pub package: PackageMeta,
    pub dependencies: Dependencies,
    pub workspace: WorkspaceDefaults,
    pub compatibility: Option<Compatibility>,
    pub permissions: Vec<String>,
    pub plugins: Vec<String>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Compatibility {
    pub min_agent_version: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: AuthorInfo,
    pub license: Option<String>,
    pub icon: Option<String>,
    pub homepage: Option<String>,
    #[serde(rename = "$schema")]
    pub schema: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct AuthorInfo {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct Dependencies {
    pub skills: std::collections::HashMap<String, String>,
    pub mcp: std::collections::HashMap<String, String>,
}

#[derive(Debug, Default, serde::Deserialize, serde::Serialize)]
pub struct WorkspaceDefaults {
    pub default_model: Option<String>,
    pub mode: Option<String>,
    pub features: Vec<String>,
}

pub struct AgentPackage {
    pub manifest: Manifest,
    pub path: PathBuf,
}

impl AgentPackage {
    /// 从当前工作区打包成 .agent 文件
    pub fn pack(workspace_dir: &Path, output_path: &Path) -> Result<Self> {
        let manifest_path = workspace_dir.join("manifest.json");
        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(&manifest_path)
                .context("读取 manifest.json 失败")?
        ).context("解析 manifest.json 失败")?;

        // 验证包内容
        Self::validate_manifest(&manifest)?;

        // 创建 ZIP 文件
        let file = std::fs::File::create(output_path)?;
        let mut zip = ZipWriter::new(file);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        // 递归添加工作区文件（排除 API Key 等敏感文件）
        Self::add_dir_to_zip(&mut zip, workspace_dir, workspace_dir, &options)?;
        zip.finish()?;

        // 计算 SHA256
        let hash = sha256_file(output_path)?;
        println!("打包成功: {} (SHA256: {})", output_path.display(), hash);

        Ok(Self { manifest, path: output_path.to_path_buf() })
    }

    /// 解包 .agent 文件并安装到指定目录
    pub fn unpack(agent_path: &Path, install_dir: &Path) -> Result<Self> {
        // 1. SHA256 校验（如果有 .sha256 文件）
        let sha256_path = agent_path.with_extension("agent.sha256");
        if sha256_path.exists() {
            let expected = std::fs::read_to_string(&sha256_path)?.trim().to_string();
            let actual = sha256_file(agent_path)?;
            anyhow::ensure!(expected == actual, "SHA256 校验失败：包可能已损坏");
        }

        // 2. 解压
        let file = std::fs::File::open(agent_path)?;
        let mut archive = ZipArchive::new(file)?;
        std::fs::create_dir_all(install_dir)?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let out_path = install_dir.join(entry.name());
            if entry.is_dir() {
                std::fs::create_dir_all(&out_path)?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out_file = std::fs::File::create(&out_path)?;
                std::io::copy(&mut entry, &mut out_file)?;
            }
        }

        // 3. 解析 manifest
        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(install_dir.join("manifest.json"))?
        )?;

        Ok(Self { manifest, path: install_dir.to_path_buf() })
    }

    /// 验证包格式和内容合法性
    pub fn validate(agent_path: &Path) -> Result<Manifest> {
        let file = std::fs::File::open(agent_path)?;
        let mut archive = ZipArchive::new(file)?;

        // 必须包含 manifest.json
        let mut manifest_entry = archive.by_name("manifest.json")
            .context(".agent 包缺少 manifest.json")?;
        let mut content = String::new();
        std::io::Read::read_to_string(&mut manifest_entry, &mut content)?;
        let manifest: Manifest = serde_json::from_str(&content)
            .context("manifest.json JSON 格式错误")?;

        Self::validate_manifest(&manifest)?;
        Ok(manifest)
    }

    fn validate_manifest(manifest: &Manifest) -> Result<()> {
        anyhow::ensure!(!manifest.package.name.is_empty(), "包名不能为空");
        anyhow::ensure!(
            semver::Version::parse(&manifest.package.version).is_ok(),
            "版本号格式无效：{}",
            manifest.package.version
        );
        Ok(())
    }

    fn add_dir_to_zip(
        zip: &mut ZipWriter<std::fs::File>,
        dir: &Path,
        base: &Path,
        options: &zip::write::FileOptions,
    ) -> Result<()> {
        // 排除敏感文件和临时文件
        let excluded = [".env", "*.key", "secrets.toml", ".DS_Store", "target/"];

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let name = path.strip_prefix(base)?.to_string_lossy().to_string();

            if excluded.iter().any(|e| name.contains(e)) {
                continue;
            }

            if path.is_dir() {
                zip.add_directory(&name, *options)?;
                Self::add_dir_to_zip(zip, &path, base, options)?;
            } else {
                zip.start_file(&name, *options)?;
                let mut f = std::fs::File::open(&path)?;
                std::io::copy(&mut f, zip)?;
            }
        }
        Ok(())
    }
}

fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(path)?;
    Ok(format!("{:x}", Sha256::digest(&data)))
}
```

---

## 4. 安装流程

```
下载 .agent 文件
       │
       ▼
SHA256 校验（完整性验证）
       │
       ▼
解析 manifest.json
       │
       ▼
检查依赖 Skill / MCP Server
       │
  ┌────┴────┐
  │  缺失依赖 │
  └────┬────┘
       ▼
从插件市场安装缺失 Skill
       │
       ▼
解压到 ~/.astro/workspaces/<name>/
       │
       ▼
创建工作区（数据库记录）
       │
       ▼
跳转到新工作区
```

---

## 5. 官方精选目录

精选目录以 `catalog.toml` 文件托管在 GitHub，定期审核更新。

```toml
# catalog.toml

[[agents]]
name = "code-assistant"
version = "1.2.0"
description = "代码审查、测试生成、重构建议全能助手"
category = "development"
tags = ["code", "testing", "refactor"]
download_url = "https://releases.astro-agent.io/agents/code-assistant-1.2.0.agent"
sha256 = "a3f8b2c1..."
installs = 12500
rating = 4.8

[[agents]]
name = "research-assistant"
version = "0.8.0"
description = "学术研究助手：文献检索、摘要、引用管理"
category = "research"
tags = ["research", "academic", "writing"]
download_url = "https://releases.astro-agent.io/agents/research-assistant-0.8.0.agent"
sha256 = "d9e7f3a2..."
installs = 8300
rating = 4.6

[[agents]]
name = "writing-assistant"
version = "1.0.0"
description = "写作助手：文章润色、结构优化、多语言翻译"
category = "writing"
tags = ["writing", "editing", "translation"]
download_url = "https://releases.astro-agent.io/agents/writing-assistant-1.0.0.agent"
sha256 = "b1c4e9f7..."
installs = 15200
rating = 4.9

[[agents]]
name = "data-analyst"
version = "0.5.0"
description = "数据分析师：CSV/Excel 处理、可视化、统计分析"
category = "data"
tags = ["data", "analysis", "visualization"]
download_url = "https://releases.astro-agent.io/agents/data-analyst-0.5.0.agent"
sha256 = "f2a8d3b6..."
installs = 6100
rating = 4.5
```

---

## 6. Tauri Commands

```rust
// src-tauri/src/commands/marketplace.rs

use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn pack_workspace(
    workspace_id: String,
    output_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let workspace_dir = state.get_workspace_dir(&workspace_id)
        .map_err(|e| e.to_string())?;
    let output = std::path::Path::new(&output_path);
    AgentPackage::pack(&workspace_dir, output)
        .map(|pkg| pkg.manifest.package.name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn install_agent_package(
    agent_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let path = std::path::Path::new(&agent_path);

    // 1. 验证包
    let manifest = AgentPackage::validate(path).map_err(|e| e.to_string())?;
    let workspace_name = manifest.package.name.clone();

    // 2. 安装依赖 Skill
    for (skill_name, version_req) in &manifest.dependencies.skills {
        state.skill_registry
            .ensure_installed(skill_name, version_req)
            .await
            .map_err(|e| e.to_string())?;
    }

    // 3. 解包
    let install_dir = state.workspaces_dir.join(&workspace_name);
    AgentPackage::unpack(path, &install_dir).map_err(|e| e.to_string())?;

    // 4. 创建工作区数据库记录
    let workspace_id = state.workspace_manager
        .create_from_dir(&install_dir, &manifest)
        .await
        .map_err(|e| e.to_string())?;

    Ok(workspace_id)
}

#[tauri::command]
pub async fn list_marketplace(
    category: Option<String>,
) -> Result<Vec<CatalogEntry>, String> {
    let catalog_url = "https://releases.astro-agent.io/catalog.toml";
    let content = reqwest::get(catalog_url)
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;

    let catalog: Catalog = toml::from_str(&content).map_err(|e| e.to_string())?;

    let agents = if let Some(cat) = category {
        catalog.agents.into_iter().filter(|a| a.category == cat).collect()
    } else {
        catalog.agents
    };

    Ok(agents)
}
```

---

## 7. 前端 Agent 市场界面

```tsx
// src/pages/MarketplacePage.tsx

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Star, Download, Package, Upload } from "lucide-react";

interface CatalogEntry {
  name: string;
  version: string;
  description: string;
  category: string;
  tags: string[];
  download_url: string;
  installs: number;
  rating: number;
}

const CATEGORIES = ["全部", "development", "research", "writing", "data"];

export function MarketplacePage() {
  const [agents, setAgents] = useState<CatalogEntry[]>([]);
  const [category, setCategory] = useState("全部");
  const [installing, setInstalling] = useState<string | null>(null);

  useEffect(() => {
    invoke<CatalogEntry[]>("list_marketplace", {
      category: category === "全部" ? null : category,
    }).then(setAgents);
  }, [category]);

  const handleInstall = async (entry: CatalogEntry) => {
    setInstalling(entry.name);
    try {
      // 下载到临时目录后安装
      const tempPath = await invoke<string>("download_to_temp", {
        url: entry.download_url,
        sha256: entry.sha256,
      });
      await invoke("install_agent_package", { agentPath: tempPath });
    } finally {
      setInstalling(null);
    }
  };

  const handleImportFile = async () => {
    const selected = await open({
      filters: [{ name: "Agent 包", extensions: ["agent"] }],
    });
    if (selected) {
      await invoke("install_agent_package", { agentPath: selected as string });
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold">Agent 市场</h1>
        <Button variant="outline" onClick={handleImportFile}>
          <Upload size={16} className="mr-2" />
          从文件安装
        </Button>
      </div>

      {/* 分类筛选 */}
      <div className="flex gap-2">
        {CATEGORIES.map((cat) => (
          <Button
            key={cat}
            variant={category === cat ? "default" : "outline"}
            size="sm"
            onClick={() => setCategory(cat)}
          >
            {cat}
          </Button>
        ))}
      </div>

      {/* Agent 卡片列表 */}
      <div className="grid grid-cols-2 gap-4">
        {agents.map((agent) => (
          <div key={agent.name} className="rounded-lg border p-4 flex flex-col gap-3">
            <div className="flex items-start justify-between">
              <div className="flex items-center gap-2">
                <Package size={20} className="text-primary" />
                <div>
                  <p className="font-semibold">{agent.name}</p>
                  <p className="text-xs text-muted-foreground">v{agent.version}</p>
                </div>
              </div>
              <div className="flex items-center gap-1 text-sm">
                <Star size={14} className="fill-yellow-400 text-yellow-400" />
                <span>{agent.rating.toFixed(1)}</span>
              </div>
            </div>

            <p className="text-sm text-muted-foreground line-clamp-2">
              {agent.description}
            </p>

            <div className="flex flex-wrap gap-1">
              {agent.tags.map((tag) => (
                <Badge key={tag} variant="secondary" className="text-xs">
                  {tag}
                </Badge>
              ))}
            </div>

            <div className="flex items-center justify-between mt-auto">
              <span className="text-xs text-muted-foreground">
                <Download size={12} className="inline mr-1" />
                {agent.installs.toLocaleString()} 次安装
              </span>
              <Button
                size="sm"
                onClick={() => handleInstall(agent)}
                disabled={installing === agent.name}
              >
                {installing === agent.name ? "安装中..." : "一键安装"}
              </Button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
```

---

## 8. 目录结构

```
crates/agent-core/src/marketplace/
├── mod.rs              # 模块导出
├── package.rs          # AgentPackage（打包/解包/验证）
├── catalog.rs          # 精选目录拉取与解析
├── installer.rs        # 安装流程（依赖解析 + 工作区创建）
└── tests/
    ├── pack_test.rs
    └── install_test.rs

src/pages/
└── MarketplacePage.tsx # 前端 Agent 市场界面
```

---

## 相关文档

- [01-插件生态.md](01-插件生态.md) — 插件生态（与 Agent 市场互补）
- [05-多工作区隔离.md](../06-桌面端/05-多工作区隔离.md) — 多工作区隔离
- [04-Skills系统.md](../02-核心功能模块/04-Skills系统.md) — Skill 系统
