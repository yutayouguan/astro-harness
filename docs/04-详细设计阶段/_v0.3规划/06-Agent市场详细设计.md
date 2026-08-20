# Agent 市场详细设计

> v0.3 精简版 | 状态：规划中 | 原始设计已精简，保留 MVP 核心

---

## 1. 设计定位

Agent 市场采用本地优先架构，v0.3 聚焦最小可用流程：

- `.agent` ZIP 包格式（manifest.json + SKILL.md + assets）
- GitHub 仓库索引（单一 `catalog.json`）
- 安装：下载 -> SHA256 校验 -> 解压 -> 注册
- 卸载：删除目录 + 清理注册

---

## 2. `.agent` 包格式

```text
my-code-assistant-1.2.0.agent (ZIP)
├── manifest.json              # 必须：包元数据
├── skills/                    # 可选：SKILL.md 文件
│   ├── code-review.md
│   └── test-generator.md
├── assets/                    # 可选：图标等
│   └── icon.png
└── README.md                  # 可选
```

### manifest.json

```json
{
  "name": "code-assistant",
  "version": "1.2.0",
  "description": "代码审查与测试生成助手",
  "author": "username",
  "tags": ["code", "testing"],
  "category": "development",
  "min_app_version": "0.3.0",
  "permissions": { "tools": ["file_read", "file_write", "shell_exec"] },
  "skills": [
    { "file": "skills/code-review.md" },
    { "file": "skills/test-generator.md" }
  ]
}
```

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    pub category: String,
    pub min_app_version: String,
    pub permissions: PackagePermissions,
    pub skills: Vec<SkillRef>,
}

impl AgentManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.name.is_empty(), "包名不能为空");
        anyhow::ensure!(
            self.name.chars().all(|c| c.is_ascii_lowercase() || c == '-' || c.is_ascii_digit()),
            "包名只允许小写字母、数字和连字符"
        );
        anyhow::ensure!(semver::Version::parse(&self.version).is_ok(), "版本号无效");
        for s in &self.skills {
            anyhow::ensure!(!s.file.contains(".."), "路径穿越: {}", s.file);
        }
        Ok(())
    }
}
```

---

## 3. GitHub 目录索引

单一 `catalog.json` 作为包发现来源。

```rust
pub struct GitHubCatalog {
    catalog_url: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub tags: Vec<String>,
    pub download_url: String,
    pub sha256: String,
    pub size_bytes: u64,
}

impl GitHubCatalog {
    pub async fn fetch_all(&self) -> anyhow::Result<Vec<CatalogEntry>> {
        let resp = self.http_client.get(&self.catalog_url).send().await?;
        Ok(resp.json().await?)
    }
}
```

---

## 4. 安装流程（4 步）

```text
1. 下载    → ~/.astro/marketplace/cache/<name>-<ver>.agent
2. SHA256  → 哈希对比，失败删除 + 报错
3. 解压    → ~/.astro/marketplace/installed/<name>/
4. 注册    → 解析 manifest → Skills 注册到 SkillRegistry → 记录 DB
```

```rust
pub struct InstallManager {
    cache_dir: PathBuf,
    installed_dir: PathBuf,
    skill_registry: Arc<RwLock<SkillRegistry>>,
    db: SqlitePool,
}

impl InstallManager {
    pub async fn install(&self, url: &str, expected_sha256: &str) -> anyhow::Result<String> {
        // 1. 下载
        let cache_path = self.cache_dir.join(url.rsplit('/').next().unwrap_or("pkg.agent"));
        let bytes = reqwest::get(url).await?.bytes().await?;
        std::fs::write(&cache_path, &bytes)?;

        // 2. SHA256
        let actual = sha256_file(&cache_path)?;
        anyhow::ensure!(actual == expected_sha256, "SHA-256 校验失败");

        // 3. 解压 + 解析
        let manifest = self.unpack_and_parse(&cache_path).await?;

        // 4. 注册 Skills
        let install_path = self.installed_dir.join(&manifest.name);
        for skill_ref in &manifest.skills {
            let content = std::fs::read_to_string(install_path.join(&skill_ref.file))?;
            let sm = SkillManifest::from_markdown(&content, &install_path.join(&skill_ref.file))?;
            self.skill_registry.write().await.update(sm, &content);
        }
        self.record_installation(&manifest).await?;
        Ok(manifest.name)
    }
}
```

---

## 5. 卸载流程

```rust
impl InstallManager {
    pub async fn uninstall(&self, name: &str) -> anyhow::Result<()> {
        let path = self.installed_dir.join(name);
        anyhow::ensure!(path.exists(), "包 {} 未安装", name);

        let manifest_str = std::fs::read_to_string(path.join("manifest.json"))?;
        let manifest = AgentManifest::from_json(&manifest_str)?;

        // 从 SkillRegistry 移除
        for skill_ref in &manifest.skills {
            let sp = path.join(&skill_ref.file);
            let content = std::fs::read_to_string(&sp).unwrap_or_default();
            if let Ok(sm) = SkillManifest::from_markdown(&content, &sp) {
                self.skill_registry.write().await.retire(&sm.id);
            }
        }

        sqlx::query!("DELETE FROM installed_agents WHERE name = ?", name)
            .execute(&self.db).await?;
        std::fs::remove_dir_all(&path)?;
        Ok(())
    }
}
```

---

## 6. 数据库表

```sql
CREATE TABLE IF NOT EXISTS installed_agents (
    name         TEXT PRIMARY KEY,
    version      TEXT NOT NULL,
    author       TEXT NOT NULL,
    installed_at INTEGER NOT NULL
);
```

---

## 7. v0.4+ 远期能力（本版本不实现）

Ed25519 签名 + Merkle 树、CatalogSource trait 多注册源、评分系统、版本迁移脚本、发布者 lint/sign/pack 管线、开发者密钥管理、WASM 插件能力审计、权限审批弹窗。

---

## 相关文档

- [01-Skills系统详细设计.md](01-Skills系统详细设计.md) -- SKILL.md 格式、SkillRegistry
- [02-Agent市场.md](../../03-系统设计阶段/09-生态扩展/02-Agent市场.md) -- 系统设计原始文档
