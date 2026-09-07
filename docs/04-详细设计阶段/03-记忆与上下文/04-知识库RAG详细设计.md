# 知识库 RAG 详细设计

> 版本：v1.0 | 日期：2026-08-09 | 状态：草稿
> 对应需求：F-05 知识库管理与 RAG 检索
> 补充文档：[08-知识库RAG.md](../../03-系统设计阶段/02-核心功能模块/08-知识库RAG.md)（整体架构、分块策略总览、前端页面）

本文档在系统设计基础上展开实现级细节：知识源数据模型、文档导入管道的 trait 抽象与各格式实现、递归分块算法、向量化与 sqlite-vec 存储、BM25+向量混合检索的 RRF 融合算法、`knowledge_manage` 工具的完整实现、检索结果的上下文注入策略、增量更新机制、Tauri Commands 以及前端组件设计。

---

## 1. 系统概述与职责边界

知识库 RAG 模块为 Agent 提供基于私有文档的检索增强生成能力。用户将文档、URL、代码文件导入工作区知识库，系统自动完成解析、分块、向量化；Agent 在对话过程中通过混合检索获取相关片段并注入上下文，实现基于私有知识的精准回答。

### 1.1 与相邻模块的职责划分

```text
┌─────────────────────────────────────────────────────────────────┐
│                        Agent 上下文窗口                          │
│                                                                 │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────┐    │
│  │  记忆系统     │  │ 知识库 RAG    │  │   全局搜索          │    │
│  │  (Memory)    │  │ (Knowledge)  │  │   (GlobalSearch)   │    │
│  │              │  │              │  │                    │    │
│  │ L1 情节记忆   │  │ 用户主动导入  │  │ 跨对话/文件/知识库  │    │
│  │ L2 语义记忆   │  │ 的外部文档    │  │ 的统一检索 UI       │    │
│  │ L3 持久记忆   │  │              │  │                    │    │
│  │ L4 程序性记忆  │  │ Agent 自动   │  │ session_search     │    │
│  │              │  │ 检索注入上下文 │  │ 工具（FTS5 对话）   │    │
│  └──────────────┘  └──────────────┘  └────────────────────┘    │
│                                                                 │
│  职责边界：                                                      │
│  · 记忆系统 — Agent 从对话中自动提取的用户知识/偏好/经验          │
│  · 知识库 RAG — 用户主动上传的外部文档，Agent 按需检索            │
│  · 全局搜索 — 面向用户的统一搜索 UI，知识库作为其搜索源之一       │
└─────────────────────────────────────────────────────────────────┘
```

| 维度 | 记忆系统 | 知识库 RAG | 全局搜索 |
| ---- | -------- | ---------- | -------- |
| 数据来源 | Agent 自动从对话中蒸馏 | 用户主动导入文档/URL | 聚合所有索引 |
| 写入方式 | `memory` 工具 | `knowledge_manage` 工具 / UI 上传 | 只读 |
| 检索触发 | 回忆触发词 / 自适应 | Agent 每轮自动检索 | 用户 `Cmd+Shift+F` |
| 注入位置 | `<memory>` 块 | `<knowledge>` 块 | 不注入上下文 |
| 存储隔离 | 按 `workspace_id` | 按 `workspace_id` | 跨源聚合 |

### 1.2 模块在系统中的位置

```text
用户 / 前端 UI
    │
    ├── KnowledgePanel（上传/管理/预览）
    │       │
    │       ▼
    │   Tauri Commands（add_source / remove_source / search_knowledge）
    │       │
    │       ▼
    ├── knowledge_manage 工具（Agent 调用入口）
    │       │
    │       ▼
    │   KnowledgeService（业务编排层）
    │       │
    │       ├── KnowledgeIngestor（文档解析 + 分块）
    │       ├── EmbeddingClient（向量化）
    │       ├── KnowledgeRepo（数据持久化）
    │       └── HybridRetriever（混合检索）
    │               │
    │               ▼
    │           sqlite-vec + FTS5（索引层）
    │
    └── ContextManager（检索结果注入上下文窗口）
```

---

## 2. 知识库数据模型

### 2.1 核心表结构

```sql
-- migrations/0009_knowledge_base.sql

-- 知识源表：记录导入的每个文档/URL
CREATE TABLE IF NOT EXISTS knowledge_sources (
    id            TEXT    PRIMARY KEY,                -- UUID v4
    workspace_id  TEXT    NOT NULL,                   -- 所属工作区
    source_type   TEXT    NOT NULL                    -- 'markdown' | 'pdf' | 'txt' | 'url' | 'code'
                  CHECK(source_type IN ('markdown', 'pdf', 'txt', 'url', 'code')),
    uri           TEXT    NOT NULL,                   -- 本地文件路径 或 URL
    title         TEXT    NOT NULL,                   -- 文档标题（文件名 / 页面标题）
    content_hash  TEXT,                               -- SHA-256，用于增量更新判断
    file_size     INTEGER,                            -- 原始文件字节数
    chunk_count   INTEGER NOT NULL DEFAULT 0,         -- 分块数量
    status        TEXT    NOT NULL DEFAULT 'pending'  -- 'pending' | 'processing' | 'ready' | 'failed'
                  CHECK(status IN ('pending', 'processing', 'ready', 'failed')),
    error_message TEXT,                               -- 失败原因
    created_at    INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER)),
    updated_at    INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER)),
    FOREIGN KEY (workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_knowledge_sources_workspace
    ON knowledge_sources(workspace_id, status);

CREATE INDEX IF NOT EXISTS idx_knowledge_sources_hash
    ON knowledge_sources(workspace_id, content_hash);

-- 知识分块表：存储文档切分后的文本片段
CREATE TABLE IF NOT EXISTS knowledge_chunks (
    id            TEXT    PRIMARY KEY,                -- UUID v4
    source_id     TEXT    NOT NULL,                   -- 所属知识源
    workspace_id  TEXT    NOT NULL,                   -- 冗余，避免 JOIN
    chunk_index   INTEGER NOT NULL,                   -- 分块序号（0-based）
    content       TEXT    NOT NULL,                   -- 分块原文
    token_count   INTEGER NOT NULL,                   -- Token 数（tiktoken cl100k_base）
    heading_path  TEXT,                               -- 标题路径（如 "# 架构 > ## 分层"）
    parent_content TEXT,                              -- 父块原文（ParentChild 策略）
    metadata_json TEXT,                               -- JSON：页码、行号、语言等
    embedding     BLOB,                               -- 向量（f32 × dim，小端序）
    created_at    INTEGER NOT NULL DEFAULT (CAST(unixepoch('now','subsec') * 1000 AS INTEGER)),
    FOREIGN KEY (source_id) REFERENCES knowledge_sources(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_knowledge_chunks_source
    ON knowledge_chunks(source_id);

CREATE INDEX IF NOT EXISTS idx_knowledge_chunks_workspace
    ON knowledge_chunks(workspace_id);
```

### 2.2 FTS5 全文索引

```sql
-- 知识分块全文索引（BM25 检索用）
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content,                         -- 分块原文
    heading_path,                    -- 标题路径，提升标题命中权重
    content='knowledge_chunks',      -- 影子表关联
    content_rowid='rowid',
    tokenize='unicode61'             -- 支持中英文混合分词
);

-- 同步触发器：INSERT / DELETE / UPDATE
CREATE TRIGGER IF NOT EXISTS chunks_fts_insert
AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.rowid, new.content, new.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_delete
AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.rowid, old.content, old.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_update
AFTER UPDATE OF content ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.rowid, old.content, old.heading_path);
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.rowid, new.content, new.heading_path);
END;
```

### 2.3 Rust 数据结构

```rust
// crates/agent-core/src/knowledge/models.rs

use serde::{Deserialize, Serialize};

/// 知识源类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Markdown,
    Pdf,
    Txt,
    Url,
    Code,
}

impl SourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Pdf => "pdf",
            Self::Txt => "txt",
            Self::Url => "url",
            Self::Code => "code",
        }
    }

    /// 根据文件扩展名自动推断类型
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "md" | "markdown" => Some(Self::Markdown),
            "pdf" => Some(Self::Pdf),
            "txt" | "text" => Some(Self::Txt),
            "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java"
            | "c" | "cpp" | "h" | "hpp" => Some(Self::Code),
            _ => None,
        }
    }
}

/// 知识源状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    Pending,
    Processing,
    Ready,
    Failed,
}

/// 知识源记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSource {
    pub id: String,
    pub workspace_id: String,
    pub source_type: SourceType,
    pub uri: String,
    pub title: String,
    pub content_hash: Option<String>,
    pub file_size: Option<i64>,
    pub chunk_count: i64,
    pub status: SourceStatus,
    pub error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 知识分块
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeChunk {
    pub id: String,
    pub source_id: String,
    pub workspace_id: String,
    pub chunk_index: i64,
    pub content: String,
    pub token_count: i64,
    pub heading_path: Option<String>,
    pub parent_content: Option<String>,
    pub metadata_json: Option<String>,
    pub created_at: i64,
}

/// 分块元数据（序列化为 metadata_json）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChunkMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,       // 代码文件的编程语言
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function_name: Option<String>,  // 代码文件的函数/类名
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_title: Option<String>,   // 所属文档标题
}
```

---

## 3. 文档导入管道

### 3.1 KnowledgeIngestor trait

文档导入管道通过 trait 抽象解耦解析逻辑与存储逻辑，每种文档格式实现独立的 ingestor。

```rust
// crates/agent-core/src/knowledge/ingestor.rs

use anyhow::Result;
use async_trait::async_trait;

/// 文档解析结果
#[derive(Debug)]
pub struct ParsedDocument {
    pub title: String,
    pub content: String,           // 提取的纯文本内容
    pub source_type: SourceType,
    pub file_size: usize,
    pub metadata: ChunkMetadata,   // 文档级元数据
}

/// 文档解析器 trait
#[async_trait]
pub trait KnowledgeIngestor: Send + Sync {
    /// 从 URI（本地路径或 URL）解析文档，返回纯文本内容
    async fn parse(&self, uri: &str) -> Result<ParsedDocument>;

    /// 判断该解析器是否能处理给定的 URI
    fn can_handle(&self, uri: &str, source_type: &SourceType) -> bool;
}

/// 解析器注册表：按优先级尝试匹配
pub struct IngestorRegistry {
    ingestors: Vec<Box<dyn KnowledgeIngestor>>,
}

impl IngestorRegistry {
    pub fn new() -> Self {
        Self {
            ingestors: vec![
                Box::new(MarkdownIngestor),
                Box::new(PdfIngestor),
                Box::new(PlainTextIngestor),
                Box::new(UrlIngestor::new()),
                Box::new(CodeIngestor::new()),
            ],
        }
    }

    pub async fn parse(&self, uri: &str, source_type: &SourceType) -> Result<ParsedDocument> {
        for ingestor in &self.ingestors {
            if ingestor.can_handle(uri, source_type) {
                return ingestor.parse(uri).await;
            }
        }
        anyhow::bail!("no ingestor found for uri={uri}, type={source_type:?}")
    }
}
```

### 3.2 Markdown 解析器

```rust
// crates/agent-core/src/knowledge/ingestors/markdown.rs

pub struct MarkdownIngestor;

#[async_trait]
impl KnowledgeIngestor for MarkdownIngestor {
    async fn parse(&self, uri: &str) -> Result<ParsedDocument> {
        let path = Path::new(uri);
        let content = tokio::fs::read_to_string(path).await?;
        let file_size = tokio::fs::metadata(path).await?.len() as usize;
        let title = extract_markdown_title(&content)
            .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());

        Ok(ParsedDocument {
            title,
            content, // Markdown 保留原文，分块器利用标题层级信息
            source_type: SourceType::Markdown,
            file_size,
            metadata: ChunkMetadata::default(),
        })
    }

    fn can_handle(&self, _uri: &str, source_type: &SourceType) -> bool {
        *source_type == SourceType::Markdown
    }
}

/// 从 Markdown 内容中提取第一个 # 标题作为文档标题
fn extract_markdown_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("# ") {
            return Some(title.trim().to_string());
        }
    }
    None
}
```

### 3.3 PDF 解析器

```rust
// crates/agent-core/src/knowledge/ingestors/pdf.rs

use pdf_extract::extract_text;

pub struct PdfIngestor;

#[async_trait]
impl KnowledgeIngestor for PdfIngestor {
    async fn parse(&self, uri: &str) -> Result<ParsedDocument> {
        let path = Path::new(uri);
        let file_size = tokio::fs::metadata(path).await?.len() as usize;

        // PDF 解析是 CPU 密集型，放到阻塞线程
        let path_owned = path.to_path_buf();
        let content = tokio::task::spawn_blocking(move || {
            extract_text(&path_owned)
        })
        .await??;

        let title = path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        Ok(ParsedDocument {
            title,
            content,
            source_type: SourceType::Pdf,
            file_size,
            metadata: ChunkMetadata::default(),
        })
    }

    fn can_handle(&self, _uri: &str, source_type: &SourceType) -> bool {
        *source_type == SourceType::Pdf
    }
}
```

### 3.4 URL 解析器（网页正文提取）

```rust
// crates/agent-core/src/knowledge/ingestors/url.rs

use reqwest::Client;
use scraper::{Html, Selector};

pub struct UrlIngestor {
    client: Client,
}

impl UrlIngestor {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent("AstroAgent/1.0")
                .build()
                .expect("failed to build HTTP client"),
        }
    }
}

#[async_trait]
impl KnowledgeIngestor for UrlIngestor {
    async fn parse(&self, uri: &str) -> Result<ParsedDocument> {
        let response = self.client.get(uri).send().await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("HTTP {status} for {uri}");
        }
        let html_text = response.text().await?;
        let file_size = html_text.len();

        let document = Html::parse_document(&html_text);

        // 提取页面标题
        let title_selector = Selector::parse("title").unwrap();
        let title = document
            .select(&title_selector)
            .next()
            .map(|el| el.inner_html().trim().to_string())
            .unwrap_or_else(|| uri.to_string());

        // 提取正文内容（article > main > body 优先级）
        let content = extract_readable_content(&document);

        Ok(ParsedDocument {
            title,
            content,
            source_type: SourceType::Url,
            file_size,
            metadata: ChunkMetadata::default(),
        })
    }

    fn can_handle(&self, uri: &str, source_type: &SourceType) -> bool {
        *source_type == SourceType::Url || uri.starts_with("http://") || uri.starts_with("https://")
    }
}

/// 按优先级尝试 <article> / <main> / <body>，提取文本并去除导航/广告
fn extract_readable_content(document: &Html) -> String {
    let selectors = ["article", "main", "body"];
    for sel_str in &selectors {
        let sel = Selector::parse(sel_str).unwrap();
        if let Some(element) = document.select(&sel).next() {
            let text: String = element
                .text()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            if text.len() > 100 {
                return text;
            }
        }
    }
    String::new()
}
```

### 3.5 代码文件解析器

```rust
// crates/agent-core/src/knowledge/ingestors/code.rs

pub struct CodeIngestor {
    // tree-sitter 解析器按语言延迟加载
    parsers: RwLock<HashMap<String, tree_sitter::Parser>>,
}

impl CodeIngestor {
    pub fn new() -> Self {
        Self {
            parsers: RwLock::new(HashMap::new()),
        }
    }

    /// 推断编程语言
    fn detect_language(path: &Path) -> Option<String> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| match ext {
                "rs" => "rust",
                "py" => "python",
                "ts" | "tsx" => "typescript",
                "js" | "jsx" => "javascript",
                "go" => "go",
                "java" => "java",
                "c" | "h" => "c",
                "cpp" | "hpp" => "cpp",
                _ => ext,
            })
            .map(String::from)
    }
}

#[async_trait]
impl KnowledgeIngestor for CodeIngestor {
    async fn parse(&self, uri: &str) -> Result<ParsedDocument> {
        let path = Path::new(uri);
        let content = tokio::fs::read_to_string(path).await?;
        let file_size = content.len();
        let language = Self::detect_language(path);

        let title = path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        Ok(ParsedDocument {
            title,
            content,
            source_type: SourceType::Code,
            file_size,
            metadata: ChunkMetadata {
                language,
                ..Default::default()
            },
        })
    }

    fn can_handle(&self, _uri: &str, source_type: &SourceType) -> bool {
        *source_type == SourceType::Code
    }
}
```

### 3.6 导入管道编排

```rust
// crates/agent-core/src/knowledge/service.rs

pub struct KnowledgeService {
    repo: KnowledgeRepo,
    registry: IngestorRegistry,
    chunker: RecursiveChunker,
    embedder: Arc<dyn EmbeddingClient>,
}

impl KnowledgeService {
    /// 完整导入流程：解析 → 分块 → 向量化 → 存储
    pub async fn ingest(
        &self,
        workspace_id: &str,
        uri: &str,
        source_type: SourceType,
    ) -> Result<String> {
        let source_id = uuid::Uuid::new_v4().to_string();

        // 1. 创建知识源记录（status=processing）
        self.repo.create_source(&KnowledgeSource {
            id: source_id.clone(),
            workspace_id: workspace_id.to_string(),
            source_type: source_type.clone(),
            uri: uri.to_string(),
            title: String::new(), // 解析后更新
            content_hash: None,
            file_size: None,
            chunk_count: 0,
            status: SourceStatus::Processing,
            error_message: None,
            created_at: now_ms(),
            updated_at: now_ms(),
        }).await?;

        // 2. 解析文档
        let parsed = match self.registry.parse(uri, &source_type).await {
            Ok(doc) => doc,
            Err(e) => {
                self.repo.update_status(&source_id, SourceStatus::Failed, Some(&e.to_string())).await?;
                return Err(e);
            }
        };

        // 3. 计算内容哈希（增量更新用）
        let content_hash = sha256_hex(&parsed.content);

        // 4. 分块
        let chunks = self.chunker.chunk(&parsed.content, &parsed.metadata);

        // 5. 批量向量化（32 条一批）
        let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings = self.embedder.embed_batch(&contents).await?;

        // 6. 事务写入分块 + 向量
        let mut tx = self.repo.begin().await?;

        for (i, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
            let chunk_id = uuid::Uuid::new_v4().to_string();
            let embedding_blob = embedding_to_blob(embedding);
            let token_count = chunk.token_count as i64;

            self.repo.insert_chunk_in_tx(&mut tx, &KnowledgeChunk {
                id: chunk_id,
                source_id: source_id.clone(),
                workspace_id: workspace_id.to_string(),
                chunk_index: i as i64,
                content: chunk.content.clone(),
                token_count,
                heading_path: chunk.heading_path.clone(),
                parent_content: chunk.parent_content.clone(),
                metadata_json: serde_json::to_string(&chunk.metadata).ok(),
                created_at: now_ms(),
            }, &embedding_blob).await?;
        }

        // 7. 更新知识源记录
        self.repo.update_source_in_tx(&mut tx, &source_id, &parsed.title,
            &content_hash, parsed.file_size as i64, chunks.len() as i64,
            SourceStatus::Ready).await?;

        tx.commit().await?;

        Ok(source_id)
    }
}

/// f32 向量转小端字节序 BLOB
fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// BLOB 转 f32 向量
fn blob_to_embedding(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

/// SHA-256 哈希
fn sha256_hex(content: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
```

---

## 4. 分块算法详细设计

### 4.1 RecursiveChunker 设计理念

递归文本分块器（RecursiveChunker）按照 **标题 > 段落 > 句子 > Token** 的层级递归切分文本。优先在高层级边界切分以保持语义完整性，仅当单个段落仍超出 `max_tokens` 限制时才降级到更低层级。

```text
输入文本
    │
    ▼
按标题分割（# / ## / ### / ####）
    │
    ├── 标题段 ≤ max_tokens？──── 是 → 直接输出为 chunk
    │
    └── 否 → 按段落分割（\n\n）
              │
              ├── 段落 ≤ max_tokens？──── 是 → 合并相邻段落至上限，输出
              │
              └── 否 → 按句子分割（。！？.\n）
                        │
                        ├── 句子组 ≤ max_tokens？──── 是 → 合并，输出
                        │
                        └── 否 → 按 Token 滑动窗口硬切
```

### 4.2 核心数据结构

```rust
// crates/agent-core/src/knowledge/chunker.rs

use tiktoken_rs::CoreBPE;

/// 分块器配置
#[derive(Debug, Clone)]
pub struct ChunkerConfig {
    pub max_tokens: usize,        // 单块最大 Token 数，默认 512
    pub overlap_tokens: usize,    // 重叠 Token 数，默认 64
    pub min_chunk_tokens: usize,  // 最小块 Token 数，低于此值与前块合并，默认 32
}

impl Default for ChunkerConfig {
    fn default() -> Self {
        Self {
            max_tokens: 512,
            overlap_tokens: 64,
            min_chunk_tokens: 32,
        }
    }
}

/// 分块结果
#[derive(Debug, Clone)]
pub struct ChunkOutput {
    pub content: String,
    pub token_count: usize,
    pub heading_path: Option<String>,     // 该块所属的标题路径
    pub parent_content: Option<String>,   // 父块原文（ParentChild 策略时填充）
    pub metadata: ChunkMetadata,
}

/// 递归文本分块器
pub struct RecursiveChunker {
    config: ChunkerConfig,
    tokenizer: CoreBPE,
}
```

### 4.3 RecursiveChunker 实现

```rust
// crates/agent-core/src/knowledge/chunker.rs

impl RecursiveChunker {
    pub fn new(config: ChunkerConfig) -> Self {
        let tokenizer = tiktoken_rs::cl100k_base().expect("failed to load cl100k_base tokenizer");
        Self { config, tokenizer }
    }

    pub fn with_defaults() -> Self {
        Self::new(ChunkerConfig::default())
    }

    /// 计算文本的 Token 数
    fn count_tokens(&self, text: &str) -> usize {
        self.tokenizer.encode_with_special_tokens(text).len()
    }

    /// 入口：对文档全文执行递归分块
    pub fn chunk(&self, text: &str, base_metadata: &ChunkMetadata) -> Vec<ChunkOutput> {
        let sections = self.split_by_headings(text);
        let mut chunks = Vec::new();

        for section in sections {
            let section_tokens = self.count_tokens(&section.content);

            if section_tokens <= self.config.max_tokens {
                // 整个标题段可以作为一个 chunk
                if section_tokens >= self.config.min_chunk_tokens {
                    chunks.push(ChunkOutput {
                        content: section.content.clone(),
                        token_count: section_tokens,
                        heading_path: Some(section.heading_path.clone()),
                        parent_content: None,
                        metadata: base_metadata.clone(),
                    });
                } else if let Some(last) = chunks.last_mut() {
                    // 太短，合并到前一个 chunk
                    last.content.push_str("\n\n");
                    last.content.push_str(&section.content);
                    last.token_count = self.count_tokens(&last.content);
                } else {
                    chunks.push(ChunkOutput {
                        content: section.content.clone(),
                        token_count: section_tokens,
                        heading_path: Some(section.heading_path.clone()),
                        parent_content: None,
                        metadata: base_metadata.clone(),
                    });
                }
            } else {
                // 标题段太长，降级到段落分割
                let sub_chunks = self.split_by_paragraphs(
                    &section.content,
                    &section.heading_path,
                    base_metadata,
                );
                chunks.extend(sub_chunks);
            }
        }

        self.apply_overlap(&mut chunks);
        chunks
    }

    /// 第一级：按 Markdown 标题（# ~ ####）分割
    fn split_by_headings(&self, text: &str) -> Vec<HeadingSection> {
        let mut sections = Vec::new();
        let mut current_content = String::new();
        let mut heading_stack: Vec<(usize, String)> = Vec::new(); // (级别, 标题文本)

        for line in text.lines() {
            let trimmed = line.trim();

            if let Some(level) = detect_heading_level(trimmed) {
                // 保存当前段
                if !current_content.trim().is_empty() {
                    sections.push(HeadingSection {
                        heading_path: build_heading_path(&heading_stack),
                        content: current_content.trim().to_string(),
                    });
                }
                current_content = String::new();

                // 更新标题栈
                let title = trimmed.trim_start_matches('#').trim().to_string();
                while heading_stack.last().map_or(false, |(l, _)| *l >= level) {
                    heading_stack.pop();
                }
                heading_stack.push((level, title));
            }

            current_content.push_str(line);
            current_content.push('\n');
        }

        // 最后一段
        if !current_content.trim().is_empty() {
            sections.push(HeadingSection {
                heading_path: build_heading_path(&heading_stack),
                content: current_content.trim().to_string(),
            });
        }

        // 无标题时整篇作为一个 section
        if sections.is_empty() {
            sections.push(HeadingSection {
                heading_path: String::new(),
                content: text.to_string(),
            });
        }

        sections
    }

    /// 第二级：按段落（\n\n）分割，合并相邻短段落
    fn split_by_paragraphs(
        &self,
        text: &str,
        heading_path: &str,
        metadata: &ChunkMetadata,
    ) -> Vec<ChunkOutput> {
        let paragraphs: Vec<&str> = text.split("\n\n")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let mut chunks = Vec::new();
        let mut buffer = String::new();
        let mut buffer_tokens = 0;

        for para in paragraphs {
            let para_tokens = self.count_tokens(para);

            if para_tokens > self.config.max_tokens {
                // 先把 buffer 里的内容输出
                if !buffer.trim().is_empty() {
                    chunks.push(ChunkOutput {
                        content: buffer.trim().to_string(),
                        token_count: buffer_tokens,
                        heading_path: Some(heading_path.to_string()),
                        parent_content: None,
                        metadata: metadata.clone(),
                    });
                    buffer = String::new();
                    buffer_tokens = 0;
                }
                // 段落本身太长，降级到句子分割
                let sub_chunks = self.split_by_sentences(para, heading_path, metadata);
                chunks.extend(sub_chunks);
                continue;
            }

            if buffer_tokens + para_tokens > self.config.max_tokens && !buffer.is_empty() {
                chunks.push(ChunkOutput {
                    content: buffer.trim().to_string(),
                    token_count: buffer_tokens,
                    heading_path: Some(heading_path.to_string()),
                    parent_content: None,
                    metadata: metadata.clone(),
                });
                buffer = String::new();
                buffer_tokens = 0;
            }

            if !buffer.is_empty() {
                buffer.push_str("\n\n");
            }
            buffer.push_str(para);
            buffer_tokens += para_tokens;
        }

        if !buffer.trim().is_empty() {
            chunks.push(ChunkOutput {
                content: buffer.trim().to_string(),
                token_count: buffer_tokens,
                heading_path: Some(heading_path.to_string()),
                parent_content: None,
                metadata: metadata.clone(),
            });
        }

        chunks
    }

    /// 第三级：按句子分割
    fn split_by_sentences(
        &self,
        text: &str,
        heading_path: &str,
        metadata: &ChunkMetadata,
    ) -> Vec<ChunkOutput> {
        let sentences = split_sentences(text);
        let mut chunks = Vec::new();
        let mut buffer = String::new();
        let mut buffer_tokens = 0;

        for sentence in &sentences {
            let sent_tokens = self.count_tokens(sentence);

            if sent_tokens > self.config.max_tokens {
                // 输出 buffer
                if !buffer.trim().is_empty() {
                    chunks.push(ChunkOutput {
                        content: buffer.trim().to_string(),
                        token_count: buffer_tokens,
                        heading_path: Some(heading_path.to_string()),
                        parent_content: None,
                        metadata: metadata.clone(),
                    });
                    buffer = String::new();
                    buffer_tokens = 0;
                }
                // 单个句子仍超限，降级到 Token 硬切
                let sub_chunks = self.split_by_token_window(sentence, heading_path, metadata);
                chunks.extend(sub_chunks);
                continue;
            }

            if buffer_tokens + sent_tokens > self.config.max_tokens && !buffer.is_empty() {
                chunks.push(ChunkOutput {
                    content: buffer.trim().to_string(),
                    token_count: buffer_tokens,
                    heading_path: Some(heading_path.to_string()),
                    parent_content: None,
                    metadata: metadata.clone(),
                });
                buffer = String::new();
                buffer_tokens = 0;
            }

            buffer.push_str(sentence);
            buffer_tokens += sent_tokens;
        }

        if !buffer.trim().is_empty() {
            chunks.push(ChunkOutput {
                content: buffer.trim().to_string(),
                token_count: buffer_tokens,
                heading_path: Some(heading_path.to_string()),
                parent_content: None,
                metadata: metadata.clone(),
            });
        }

        chunks
    }

    /// 第四级：按 Token 数滑动窗口硬切（最后手段）
    fn split_by_token_window(
        &self,
        text: &str,
        heading_path: &str,
        metadata: &ChunkMetadata,
    ) -> Vec<ChunkOutput> {
        let tokens = self.tokenizer.encode_with_special_tokens(text);
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < tokens.len() {
            let end = (start + self.config.max_tokens).min(tokens.len());
            let chunk_tokens = &tokens[start..end];
            let content = self.tokenizer.decode(chunk_tokens.to_vec())
                .unwrap_or_default();

            chunks.push(ChunkOutput {
                content: content.trim().to_string(),
                token_count: chunk_tokens.len(),
                heading_path: Some(heading_path.to_string()),
                parent_content: None,
                metadata: metadata.clone(),
            });

            // 滑动：步长 = max_tokens - overlap_tokens
            let stride = self.config.max_tokens.saturating_sub(self.config.overlap_tokens);
            start += stride.max(1);
        }

        chunks
    }

    /// 后处理：在相邻块之间添加文本重叠
    fn apply_overlap(&self, chunks: &mut Vec<ChunkOutput>) {
        if self.config.overlap_tokens == 0 || chunks.len() < 2 {
            return;
        }

        // 从后向前处理，避免修改影响后续索引
        for i in (1..chunks.len()).rev() {
            let prev_content = chunks[i - 1].content.clone();
            let prev_tokens = self.tokenizer.encode_with_special_tokens(&prev_content);

            if prev_tokens.len() > self.config.overlap_tokens {
                // 取前一个 chunk 末尾的 overlap_tokens 个 token
                let overlap_start = prev_tokens.len() - self.config.overlap_tokens;
                let overlap_tokens = &prev_tokens[overlap_start..];
                let overlap_text = self.tokenizer.decode(overlap_tokens.to_vec())
                    .unwrap_or_default();

                // 将重叠文本前缀添加到当前 chunk
                let new_content = format!("{}\n{}", overlap_text.trim(), chunks[i].content);
                chunks[i].token_count = self.count_tokens(&new_content);
                chunks[i].content = new_content;
            }
        }
    }
}

// ── 辅助函数 ──────────────────────────────────────────────

#[derive(Debug)]
struct HeadingSection {
    heading_path: String,
    content: String,
}

/// 检测 Markdown 标题级别（# = 1, ## = 2, ...），非标题返回 None
fn detect_heading_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') { return None; }
    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level > 0 && level <= 6 && trimmed.chars().nth(level) == Some(' ') {
        Some(level)
    } else {
        None
    }
}

/// 构建标题路径（如 "# 架构 > ## 分层"）
fn build_heading_path(stack: &[(usize, String)]) -> String {
    stack.iter()
        .map(|(level, title)| format!("{} {}", "#".repeat(*level), title))
        .collect::<Vec<_>>()
        .join(" > ")
}

/// 按句子边界切分（支持中英文标点）
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if matches!(ch, '。' | '！' | '？' | '.' | '!' | '?') {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current = String::new();
        }
    }
    if !current.trim().is_empty() {
        sentences.push(current.trim().to_string());
    }
    sentences
}
```

---

## 5. 向量化与存储

### 5.1 EmbeddingClient trait

```rust
// crates/agent-core/src/knowledge/embedding.rs

use anyhow::Result;
use async_trait::async_trait;

/// 向量维度
pub const EMBEDDING_DIM_OPENAI: usize = 1536;     // text-embedding-3-small
pub const EMBEDDING_DIM_LOCAL: usize = 768;        // nomic-embed-text

/// Embedding 生成客户端
#[async_trait]
pub trait EmbeddingClient: Send + Sync {
    /// 单条文本向量化
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// 批量向量化（建议实现内部分批，每批 ≤ 32 条）
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>>;

    /// 返回向量维度
    fn dimension(&self) -> usize;
}
```

### 5.2 OpenAI Embedding 实现

```rust
// crates/agent-core/src/knowledge/embedding_openai.rs

pub struct OpenAIEmbeddingClient {
    client: reqwest::Client,
    api_key: String,
    model: String,         // "text-embedding-3-small"
    batch_size: usize,     // 每批最大条数，默认 32
}

impl OpenAIEmbeddingClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model: "text-embedding-3-small".to_string(),
            batch_size: 32,
        }
    }
}

#[async_trait]
impl EmbeddingClient for OpenAIEmbeddingClient {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut results = self.embed_batch(&[text]).await?;
        results.pop().ok_or_else(|| anyhow::anyhow!("empty embedding response"))
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::with_capacity(texts.len());

        // 分批请求，每批 ≤ batch_size
        for batch in texts.chunks(self.batch_size) {
            let body = serde_json::json!({
                "model": self.model,
                "input": batch,
            });

            let response = self.client
                .post("https://api.openai.com/v1/embeddings")
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await?;

            let resp: EmbeddingResponse = response.json().await?;
            for item in resp.data {
                all_embeddings.push(item.embedding);
            }
        }

        Ok(all_embeddings)
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIM_OPENAI
    }
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingItem>,
}

#[derive(Deserialize)]
struct EmbeddingItem {
    embedding: Vec<f32>,
}
```

### 5.3 本地 ONNX 离线 Embedding（降级方案）

```rust
// crates/agent-core/src/knowledge/embedding_onnx.rs

use ort::{Environment, Session, Value};

pub struct OnnxEmbeddingClient {
    session: Session,
    tokenizer: tokenizers::Tokenizer,
}

impl OnnxEmbeddingClient {
    pub fn new(model_path: &Path, tokenizer_path: &Path) -> Result<Self> {
        let environment = Environment::builder()
            .with_name("knowledge_embedding")
            .build()?;
        let session = environment
            .new_session_builder()?
            .with_model_from_file(model_path)?;
        let tokenizer = tokenizers::Tokenizer::from_file(tokenizer_path)
            .map_err(|e| anyhow::anyhow!("tokenizer load failed: {e}"))?;

        Ok(Self { session, tokenizer })
    }
}

#[async_trait]
impl EmbeddingClient for OnnxEmbeddingClient {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // ONNX 推理是 CPU 密集型，在阻塞线程执行
        let text_owned = text.to_string();
        let session = &self.session;
        let tokenizer = &self.tokenizer;

        tokio::task::spawn_blocking(move || {
            let encoding = tokenizer.encode(text_owned, true)
                .map_err(|e| anyhow::anyhow!("{e}"))?;

            let input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
            let attention_mask: Vec<i64> = encoding.get_attention_mask().iter().map(|&m| m as i64).collect();
            let len = input_ids.len();

            // 构造 ONNX 输入
            let input_tensor = Value::from_array(([1, len], input_ids.as_slice()))?;
            let mask_tensor = Value::from_array(([1, len], attention_mask.as_slice()))?;

            let outputs = session.run(vec![input_tensor, mask_tensor])?;
            let embedding: Vec<f32> = outputs[0].try_extract()?.view().to_owned().into_raw_vec();

            // 均值池化
            let dim = embedding.len() / len;
            let mut pooled = vec![0.0f32; dim];
            for i in 0..len {
                for j in 0..dim {
                    pooled[j] += embedding[i * dim + j];
                }
            }
            for v in &mut pooled {
                *v /= len as f32;
            }

            // L2 归一化
            let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut pooled {
                    *v /= norm;
                }
            }

            Ok(pooled)
        })
        .await?
    }

    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimension(&self) -> usize {
        EMBEDDING_DIM_LOCAL
    }
}
```

### 5.4 Embedding 提供者选择策略

```rust
// crates/agent-core/src/knowledge/embedding.rs

pub fn create_embedding_client(config: &AppConfig) -> Arc<dyn EmbeddingClient> {
    // 优先使用 OpenAI API（需要有效的 API Key）
    if let Some(api_key) = &config.openai_api_key {
        if !api_key.is_empty() {
            return Arc::new(OpenAIEmbeddingClient::new(api_key.clone()));
        }
    }

    // 降级到本地 ONNX 模型
    let model_dir = config.data_dir.join("models");
    let model_path = model_dir.join("all-MiniLM-L6-v2.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        match OnnxEmbeddingClient::new(&model_path, &tokenizer_path) {
            Ok(client) => return Arc::new(client),
            Err(e) => tracing::warn!("ONNX model load failed, embedding disabled: {e}"),
        }
    }

    tracing::warn!("no embedding provider available, vector search will be disabled");
    Arc::new(NoopEmbeddingClient)
}
```

### 5.5 sqlite-vec 虚拟表（知识库专用）

知识库的向量存储直接使用 `knowledge_chunks.embedding` BLOB 列 + `vec_distance_cosine` 函数，不创建独立的 sqlite-vec 虚拟表。原因：

- `knowledge_chunks` 表已携带 `workspace_id` 过滤条件，独立虚拟表无法直接 JOIN 过滤
- 知识库规模（万级 chunk）下线性扫描 + 余弦距离计算性能足够（< 50ms）
- 当单工作区 chunk 数超过 10 万时，再考虑引入 sqlite-vec 的 ANN 索引

向量搜索 SQL：

```sql
SELECT kc.id, kc.content, kc.heading_path, kc.metadata_json,
       vec_distance_cosine(kc.embedding, ?) AS distance
FROM knowledge_chunks kc
WHERE kc.workspace_id = ?
  AND kc.embedding IS NOT NULL
ORDER BY distance ASC
LIMIT ?;
```

---

## 6. 混合检索算法

### 6.1 HybridRetriever 架构

```text
用户查询
    │
    ├────────────────┐
    ▼                ▼
向量检索            BM25 全文检索
(sqlite-vec)       (FTS5 chunks_fts)
    │                │
    ▼                ▼
排名列表 A          排名列表 B
(chunk_id, rank)   (chunk_id, rank)
    │                │
    └──────┬─────────┘
           ▼
    RRF 融合排序 (k=60)
           │
           ▼
    Top-K 结果（默认 k=5）
           │
           ▼
    ParentChild 扩展
  （若 chunk 有 parent_content，
    替换为父块内容）
```

### 6.2 HybridRetriever 实现

```rust
// crates/agent-core/src/knowledge/retriever.rs

use std::collections::HashMap;

/// 混合检索器
pub struct HybridRetriever {
    pool: SqlitePool,
    embedder: Arc<dyn EmbeddingClient>,
    rrf_k: f32,           // RRF 常数，默认 60.0
    vector_weight: f32,   // 向量检索权重，默认 0.5
    fts_weight: f32,      // BM25 检索权重，默认 0.5
}

impl HybridRetriever {
    pub fn new(pool: SqlitePool, embedder: Arc<dyn EmbeddingClient>) -> Self {
        Self {
            pool,
            embedder,
            rrf_k: 60.0,
            vector_weight: 0.5,
            fts_weight: 0.5,
        }
    }

    /// 混合检索入口
    pub async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<RetrievalResult>> {
        // 候选池大小：各检索引擎返回 top_k * 3 的候选，RRF 后取 top_k
        let candidate_k = top_k * 3;

        // 并发执行向量检索和 BM25 检索
        let (vector_results, fts_results) = tokio::join!(
            self.vector_search(workspace_id, query, candidate_k),
            self.fts_search(workspace_id, query, candidate_k),
        );

        let vector_results = vector_results.unwrap_or_default();
        let fts_results = fts_results.unwrap_or_default();

        // RRF 融合
        let fused = self.rrf_fusion(&vector_results, &fts_results);

        // 取 Top-K 并扩展 ParentChild
        let top_results: Vec<RetrievalResult> = fused
            .into_iter()
            .take(top_k)
            .collect();

        Ok(top_results)
    }

    /// 向量 KNN 检索
    async fn vector_search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedChunk>> {
        let query_embedding = self.embedder.embed(query).await?;
        let embedding_blob = embedding_to_blob(&query_embedding);

        let rows = sqlx::query!(
            r#"
            SELECT kc.id, kc.content, kc.heading_path, kc.parent_content,
                   kc.metadata_json, kc.source_id,
                   vec_distance_cosine(kc.embedding, ?) AS distance
            FROM knowledge_chunks kc
            WHERE kc.workspace_id = ?
              AND kc.embedding IS NOT NULL
            ORDER BY distance ASC
            LIMIT ?
            "#,
            embedding_blob,
            workspace_id,
            limit as i64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, r)| RankedChunk {
                id: r.id,
                content: r.content,
                heading_path: r.heading_path,
                parent_content: r.parent_content,
                metadata_json: r.metadata_json,
                source_id: r.source_id,
                rank,
                raw_score: 1.0 - r.distance.unwrap_or(1.0) as f32, // 余弦相似度
            })
            .collect())
    }

    /// BM25 全文检索
    async fn fts_search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedChunk>> {
        // FTS5 查询：对 content 和 heading_path 同时检索
        // heading_path 权重 ×2（标题命中更重要）
        let rows = sqlx::query!(
            r#"
            SELECT kc.id, kc.content, kc.heading_path, kc.parent_content,
                   kc.metadata_json, kc.source_id,
                   bm25(chunks_fts, 1.0, 2.0) AS bm25_score
            FROM chunks_fts
            JOIN knowledge_chunks kc ON kc.rowid = chunks_fts.rowid
            WHERE chunks_fts MATCH ?
              AND kc.workspace_id = ?
            ORDER BY bm25_score
            LIMIT ?
            "#,
            query,
            workspace_id,
            limit as i64
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(rank, r)| RankedChunk {
                id: r.id,
                content: r.content,
                heading_path: r.heading_path,
                parent_content: r.parent_content,
                metadata_json: r.metadata_json,
                source_id: r.source_id,
                rank,
                raw_score: -(r.bm25_score.unwrap_or(0.0) as f32), // BM25 越负越相关
            })
            .collect())
    }

    /// Reciprocal Rank Fusion（RRF）融合排序
    ///
    /// 公式：RRF_score(d) = Σ  w_i / (k + rank_i(d))
    ///
    /// 其中 k=60（平滑常数），rank 从 1 开始计数
    fn rrf_fusion(
        &self,
        vector_results: &[RankedChunk],
        fts_results: &[RankedChunk],
    ) -> Vec<RetrievalResult> {
        let mut scores: HashMap<String, f32> = HashMap::new();
        let mut chunks: HashMap<String, &RankedChunk> = HashMap::new();

        // 向量检索贡献分
        for chunk in vector_results {
            let rrf_score = self.vector_weight / (self.rrf_k + chunk.rank as f32 + 1.0);
            *scores.entry(chunk.id.clone()).or_default() += rrf_score;
            chunks.entry(chunk.id.clone()).or_insert(chunk);
        }

        // BM25 检索贡献分
        for chunk in fts_results {
            let rrf_score = self.fts_weight / (self.rrf_k + chunk.rank as f32 + 1.0);
            *scores.entry(chunk.id.clone()).or_default() += rrf_score;
            chunks.entry(chunk.id.clone()).or_insert(chunk);
        }

        // 按 RRF 分数降序排列
        let mut results: Vec<RetrievalResult> = scores
            .into_iter()
            .filter_map(|(id, rrf_score)| {
                let chunk = chunks.get(&id)?;
                Some(RetrievalResult {
                    chunk_id: id,
                    // 若有 parent_content 则使用父块内容（ParentChild 策略扩展）
                    content: chunk.parent_content.clone()
                        .unwrap_or_else(|| chunk.content.clone()),
                    heading_path: chunk.heading_path.clone(),
                    source_id: chunk.source_id.clone(),
                    metadata_json: chunk.metadata_json.clone(),
                    rrf_score,
                })
            })
            .collect();

        results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap_or(std::cmp::Ordering::Equal));
        results
    }
}

/// 内部排名结构
#[derive(Debug)]
struct RankedChunk {
    id: String,
    content: String,
    heading_path: Option<String>,
    parent_content: Option<String>,
    metadata_json: Option<String>,
    source_id: String,
    rank: usize,
    raw_score: f32,
}

/// 检索结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub chunk_id: String,
    pub content: String,
    pub heading_path: Option<String>,
    pub source_id: String,
    pub metadata_json: Option<String>,
    pub rrf_score: f32,
}
```

### 6.3 RRF 算法说明

Reciprocal Rank Fusion 通过倒数排名加权避免了不同检索引擎分数量纲不统一的问题。常数 k=60 来自原始论文（Cormack et al., 2009）的推荐值，作用是对排名靠后的结果施加更平滑的惩罚。

```text
RRF_score(d) = w_vec / (60 + rank_vec(d)) + w_fts / (60 + rank_fts(d))

示例（w_vec=0.5, w_fts=0.5）：
- chunk A：向量排名 1，FTS 排名 3
  RRF = 0.5/61 + 0.5/63 = 0.00820 + 0.00794 = 0.01614
- chunk B：向量排名 5，FTS 排名 1
  RRF = 0.5/65 + 0.5/61 = 0.00769 + 0.00820 = 0.01589
- chunk C：仅出现在向量排名 2
  RRF = 0.5/62 + 0 = 0.00806

排序：A > B > C（两个引擎都命中的结果排名更高）
```

---

## 7. knowledge_manage 工具实现

### 7.1 工具定义

```rust
// crates/agent-core/src/tools/knowledge_manage.rs

use crate::tools::{Tool, ToolInput, ToolOutput, RiskLevel};
use serde::{Deserialize, Serialize};

/// knowledge_manage 工具 — Agent 管理工作区知识库的入口
pub struct KnowledgeManageTool {
    service: Arc<KnowledgeService>,
}

impl Tool for KnowledgeManageTool {
    fn name(&self) -> &str {
        "knowledge_manage"
    }

    fn description(&self) -> &str {
        "管理当前工作区的知识库：添加文档/URL、删除知识源、列出已有知识源、\
         搜索知识库。添加操作会自动完成解析、分块和向量化。"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "remove", "list", "search"],
                    "description": "操作类型"
                },
                "uri": {
                    "type": "string",
                    "description": "文档路径或 URL（add 操作必填）"
                },
                "source_type": {
                    "type": "string",
                    "enum": ["markdown", "pdf", "txt", "url", "code"],
                    "description": "文档类型（add 操作可选，不填则自动推断）"
                },
                "source_id": {
                    "type": "string",
                    "description": "知识源 ID（remove 操作必填）"
                },
                "query": {
                    "type": "string",
                    "description": "搜索关键词（search 操作必填）"
                },
                "top_k": {
                    "type": "integer",
                    "default": 5,
                    "maximum": 20,
                    "description": "搜索返回的最大结果数"
                }
            },
            "required": ["operation"]
        })
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }
}
```

### 7.2 ToolInput / ToolOutput 结构

```rust
// crates/agent-core/src/tools/knowledge_manage.rs

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "lowercase")]
pub enum KnowledgeOperation {
    Add {
        uri: String,
        source_type: Option<String>,
    },
    Remove {
        source_id: String,
    },
    List {},
    Search {
        query: String,
        #[serde(default = "default_top_k")]
        top_k: usize,
    },
}

fn default_top_k() -> usize { 5 }

/// 工具输出
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum KnowledgeToolOutput {
    Added {
        source_id: String,
        title: String,
        chunk_count: usize,
        message: String,
    },
    Removed {
        source_id: String,
        message: String,
    },
    Listed {
        sources: Vec<SourceSummary>,
        total: usize,
    },
    SearchResults {
        results: Vec<SearchResultOutput>,
        total: usize,
    },
}

#[derive(Debug, Serialize)]
pub struct SourceSummary {
    pub id: String,
    pub title: String,
    pub source_type: String,
    pub chunk_count: i64,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct SearchResultOutput {
    pub content: String,
    pub source_title: String,
    pub heading_path: Option<String>,
    pub relevance_score: f32,
}
```

### 7.3 执行逻辑

```rust
// crates/agent-core/src/tools/knowledge_manage.rs

#[async_trait]
impl ToolExecutor for KnowledgeManageTool {
    async fn execute(
        &self,
        input: serde_json::Value,
        context: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let workspace_id = &context.workspace_id;
        let op: KnowledgeOperation = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(e.to_string()))?;

        match op {
            KnowledgeOperation::Add { uri, source_type } => {
                // 自动推断文档类型
                let st = if let Some(st_str) = source_type {
                    serde_json::from_value(serde_json::json!(st_str))
                        .map_err(|_| ToolError::InvalidInput("invalid source_type".into()))?
                } else {
                    self.infer_source_type(&uri)?
                };

                let source_id = self.service
                    .ingest(workspace_id, &uri, st)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let source = self.service.repo
                    .get_source(&source_id)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                Ok(ToolOutput::success_json(&KnowledgeToolOutput::Added {
                    source_id,
                    title: source.title.clone(),
                    chunk_count: source.chunk_count as usize,
                    message: format!(
                        "已添加文档「{}」，共生成 {} 个分块",
                        source.title, source.chunk_count
                    ),
                }))
            }

            KnowledgeOperation::Remove { source_id } => {
                self.service.repo
                    .delete_source(workspace_id, &source_id)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                Ok(ToolOutput::success_json(&KnowledgeToolOutput::Removed {
                    source_id: source_id.clone(),
                    message: format!("已删除知识源 {source_id}"),
                }))
            }

            KnowledgeOperation::List {} => {
                let sources = self.service.repo
                    .list_sources(workspace_id)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let summaries: Vec<SourceSummary> = sources.iter().map(|s| SourceSummary {
                    id: s.id.clone(),
                    title: s.title.clone(),
                    source_type: s.source_type.as_str().to_string(),
                    chunk_count: s.chunk_count,
                    status: format!("{:?}", s.status).to_lowercase(),
                    created_at: s.created_at,
                }).collect();

                let total = summaries.len();
                Ok(ToolOutput::success_json(&KnowledgeToolOutput::Listed {
                    sources: summaries,
                    total,
                }))
            }

            KnowledgeOperation::Search { query, top_k } => {
                let results = self.service.retriever
                    .search(workspace_id, &query, top_k)
                    .await
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

                let outputs: Vec<SearchResultOutput> = results.iter().map(|r| {
                    let source_title = r.metadata_json.as_deref()
                        .and_then(|j| serde_json::from_str::<ChunkMetadata>(j).ok())
                        .and_then(|m| m.source_title)
                        .unwrap_or_default();
                    SearchResultOutput {
                        content: r.content.clone(),
                        source_title,
                        heading_path: r.heading_path.clone(),
                        relevance_score: r.rrf_score,
                    }
                }).collect();

                let total = outputs.len();
                Ok(ToolOutput::success_json(&KnowledgeToolOutput::SearchResults {
                    results: outputs,
                    total,
                }))
            }
        }
    }
}

impl KnowledgeManageTool {
    /// 从文件扩展名或 URL 前缀推断文档类型
    fn infer_source_type(&self, uri: &str) -> Result<SourceType, ToolError> {
        if uri.starts_with("http://") || uri.starts_with("https://") {
            return Ok(SourceType::Url);
        }
        let path = Path::new(uri);
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(SourceType::from_extension)
            .ok_or_else(|| ToolError::InvalidInput(
                format!("无法推断文件类型，请指定 source_type: {uri}")
            ))
    }
}
```

---

## 8. 上下文注入

### 8.1 注入时机与触发条件

知识库检索在每轮 LLM 调用前自动执行（与记忆系统的"回忆触发词"按需检索不同），原因是知识库是用户主动导入的参考资料，Agent 应始终参考。

```text
用户消息到达
    │
    ▼
ContextManager.build_context()
    │
    ├── 1. 检查工作区是否有知识源（chunk_count > 0）
    │       │
    │       └── 无 → 跳过知识库检索
    │
    ├── 2. 对用户消息执行混合检索（top_k=5）
    │
    ├── 3. 过滤低相关度结果（rrf_score < 阈值 0.005）
    │
    ├── 4. 格式化为 <knowledge> XML 块
    │
    └── 5. 注入 system prompt 或 user message 前缀
```

### 8.2 注入格式

检索到的知识片段以结构化 XML 格式注入，便于 LLM 区分知识来源：

```rust
// crates/agent-core/src/knowledge/injector.rs

/// 将检索结果格式化为上下文注入文本
pub fn format_knowledge_context(
    results: &[RetrievalResult],
    token_budget: usize,
) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let mut output = String::from("<knowledge>\n");
    let mut used_tokens = 10; // XML 标签开销

    for (i, result) in results.iter().enumerate() {
        let chunk_text = format_single_chunk(i + 1, result);
        let chunk_tokens = estimate_tokens(&chunk_text);

        if used_tokens + chunk_tokens > token_budget {
            break; // 超出预算，停止添加
        }

        output.push_str(&chunk_text);
        used_tokens += chunk_tokens;
    }

    output.push_str("</knowledge>");

    if used_tokens <= 10 {
        return None; // 无有效内容
    }

    Some(output)
}

fn format_single_chunk(index: usize, result: &RetrievalResult) -> String {
    let heading = result.heading_path.as_deref().unwrap_or("无标题");
    format!(
        "<chunk index=\"{index}\" source=\"{source}\" heading=\"{heading}\">\n{content}\n</chunk>\n",
        source = result.source_id,
        heading = heading,
        content = result.content.trim(),
    )
}
```

注入后的 system prompt 结构示例：

```text
<system_prompt>
  ... 基础 Persona 指令 ...
  ... 工具定义 ...

  <knowledge>
  <chunk index="1" source="doc-uuid-1" heading="# 架构 > ## 分层设计">
  系统采用三层架构：表现层、业务层、数据层。表现层使用 React + Tauri...
  </chunk>
  <chunk index="2" source="doc-uuid-2" heading="# API 设计">
  RESTful API 设计规范：使用 JSON 格式，HTTP 状态码遵循 RFC 7231...
  </chunk>
  </knowledge>

  ... 记忆注入 ...
</system_prompt>
```

### 8.3 Token 预算分配

知识库注入的 token 预算从 `ContextManager` 的整体预算中分配：

```rust
// crates/agent-core/src/compression.rs (目标设计)

pub struct TokenBudget {
    pub total: usize,               // 模型窗口 × 0.90
    pub system_prompt: usize,       // 固定指令约 4,000
    pub tool_definitions: usize,    // 工具 Schema 约 3,000
    pub memory_snapshot: usize,     // 记忆注入约 1,200
    pub knowledge_context: usize,   // 知识库注入上限 2,000
    pub history_reserve: usize,     // 输出预留约 4,000
    // 剩余全部分配给对话历史
}
```

| 注入内容 | Token 预算 | 说明 |
| -------- | ---------- | ---- |
| 知识库上下文 | 2,000 tokens | 约 3~5 个 chunk（512 tokens/chunk） |
| 记忆快照 | 1,200 tokens | MEMORY.md + USER.md 冻结注入 |
| 合计占用 | 3,200 tokens | 占 200k 窗口的 1.6% |

当知识库检索结果为空（工作区无知识源或无相关结果）时，`knowledge_context` 预算归还给对话历史。

---

## 9. 增量更新与同步

### 9.1 内容变更检测

用户修改已导入的本地文件后，系统需检测变更并重新索引。采用 **内容哈希比较** 策略：

```rust
// crates/agent-core/src/knowledge/sync.rs

pub struct KnowledgeSyncManager {
    service: Arc<KnowledgeService>,
    repo: KnowledgeRepo,
}

impl KnowledgeSyncManager {
    /// 检查工作区内所有知识源是否有更新
    pub async fn check_for_updates(&self, workspace_id: &str) -> Result<Vec<UpdateAction>> {
        let sources = self.repo.list_sources(workspace_id).await?;
        let mut actions = Vec::new();

        for source in &sources {
            if source.source_type == SourceType::Url {
                // URL 类型不自动重抓，需用户手动触发
                continue;
            }

            let path = Path::new(&source.uri);
            if !path.exists() {
                actions.push(UpdateAction::SourceMissing {
                    source_id: source.id.clone(),
                    uri: source.uri.clone(),
                });
                continue;
            }

            // 读取文件并计算哈希
            let content = tokio::fs::read_to_string(path).await?;
            let new_hash = sha256_hex(&content);

            if source.content_hash.as_deref() != Some(&new_hash) {
                actions.push(UpdateAction::ContentChanged {
                    source_id: source.id.clone(),
                    uri: source.uri.clone(),
                    old_hash: source.content_hash.clone(),
                    new_hash,
                });
            }
        }

        Ok(actions)
    }

    /// 执行增量更新：删除旧分块 → 重新解析分块 → 写入新分块
    pub async fn reindex_source(&self, source_id: &str) -> Result<()> {
        let source = self.repo.get_source(source_id).await?;

        // 1. 删除旧分块（通过 ON DELETE CASCADE 自动清理 FTS 触发器）
        self.repo.delete_chunks_by_source(source_id).await?;

        // 2. 重新走完整导入管道
        let parsed = self.service.registry
            .parse(&source.uri, &source.source_type)
            .await?;
        let new_hash = sha256_hex(&parsed.content);

        let chunks = self.service.chunker.chunk(&parsed.content, &parsed.metadata);
        let contents: Vec<&str> = chunks.iter().map(|c| c.content.as_str()).collect();
        let embeddings = self.service.embedder.embed_batch(&contents).await?;

        // 3. 事务写入
        let mut tx = self.repo.begin().await?;

        for (i, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
            let chunk_id = uuid::Uuid::new_v4().to_string();
            let embedding_blob = embedding_to_blob(embedding);

            self.repo.insert_chunk_in_tx(&mut tx, &KnowledgeChunk {
                id: chunk_id,
                source_id: source_id.to_string(),
                workspace_id: source.workspace_id.clone(),
                chunk_index: i as i64,
                content: chunk.content.clone(),
                token_count: chunk.token_count as i64,
                heading_path: chunk.heading_path.clone(),
                parent_content: chunk.parent_content.clone(),
                metadata_json: serde_json::to_string(&chunk.metadata).ok(),
                created_at: now_ms(),
            }, &embedding_blob).await?;
        }

        // 4. 更新知识源元数据
        self.repo.update_source_in_tx(
            &mut tx, source_id, &parsed.title,
            &new_hash, parsed.file_size as i64, chunks.len() as i64,
            SourceStatus::Ready,
        ).await?;

        tx.commit().await?;

        tracing::info!(
            source_id = source_id,
            chunk_count = chunks.len(),
            "knowledge source reindexed"
        );

        Ok(())
    }
}

#[derive(Debug)]
pub enum UpdateAction {
    ContentChanged {
        source_id: String,
        uri: String,
        old_hash: Option<String>,
        new_hash: String,
    },
    SourceMissing {
        source_id: String,
        uri: String,
    },
}
```

### 9.2 同步调度策略

| 触发时机 | 行为 |
| -------- | ---- |
| Session 启动时 | `check_for_updates` 扫描所有本地文件知识源 |
| 文件系统监听 | `notify` crate 监听已导入文件的变更事件（debounce 5s） |
| 用户手动触发 | KnowledgePanel 中的"重新索引"按钮 |

文件监听使用 debounce 防止频繁写入导致重复索引：

```rust
// crates/agent-core/src/knowledge/watcher.rs

pub fn watch_knowledge_sources(
    workspace_id: String,
    sources: Vec<KnowledgeSource>,
    sync_manager: Arc<KnowledgeSyncManager>,
) -> Result<notify::RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;

    for source in &sources {
        if source.source_type != SourceType::Url {
            let path = Path::new(&source.uri);
            if path.exists() {
                watcher.watch(path, notify::RecursiveMode::NonRecursive)?;
            }
        }
    }

    // 后台线程处理文件变更事件
    tokio::spawn(async move {
        let mut debounce_map: HashMap<String, Instant> = HashMap::new();

        while let Ok(event) = rx.recv() {
            if let Ok(event) = event {
                for path in &event.paths {
                    let key = path.to_string_lossy().to_string();
                    let now = Instant::now();

                    // 5 秒内的重复事件忽略
                    if debounce_map.get(&key).map_or(false, |t| now.duration_since(*t) < Duration::from_secs(5)) {
                        continue;
                    }
                    debounce_map.insert(key.clone(), now);

                    // 查找对应的 source_id 并触发重新索引
                    if let Some(source) = sources.iter().find(|s| s.uri == key) {
                        if let Err(e) = sync_manager.reindex_source(&source.id).await {
                            tracing::warn!(source_id = %source.id, "reindex failed: {e}");
                        }
                    }
                }
            }
        }
    });

    Ok(watcher)
}
```

---

## 10. Tauri Commands

```rust
// apps/desktop/src-tauri/src/commands/knowledge.rs

use tauri::State;
use crate::AppState;

/// 列出工作区内所有知识源
#[tauri::command]
pub async fn list_sources(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<SourceSummary>, String> {
    let sources = state
        .knowledge_service
        .repo
        .list_sources(&workspace_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(sources
        .into_iter()
        .map(|s| SourceSummary {
            id: s.id,
            title: s.title,
            source_type: s.source_type.as_str().to_string(),
            chunk_count: s.chunk_count,
            status: format!("{:?}", s.status).to_lowercase(),
            created_at: s.created_at,
        })
        .collect())
}

/// 添加知识源（本地文件或 URL）
#[tauri::command]
pub async fn add_source(
    workspace_id: String,
    uri: String,
    source_type: Option<String>,
    state: State<'_, AppState>,
) -> Result<AddSourceResult, String> {
    let st = match source_type {
        Some(s) => serde_json::from_value(serde_json::json!(s))
            .map_err(|_| format!("invalid source_type: {s}"))?,
        None => infer_source_type_from_uri(&uri)
            .map_err(|e| e.to_string())?,
    };

    let source_id = state
        .knowledge_service
        .ingest(&workspace_id, &uri, st)
        .await
        .map_err(|e| e.to_string())?;

    let source = state
        .knowledge_service
        .repo
        .get_source(&source_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(AddSourceResult {
        source_id,
        title: source.title,
        chunk_count: source.chunk_count as usize,
    })
}

#[derive(Debug, Serialize)]
pub struct AddSourceResult {
    pub source_id: String,
    pub title: String,
    pub chunk_count: usize,
}

/// 删除知识源及其所有分块
#[tauri::command]
pub async fn remove_source(
    workspace_id: String,
    source_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .knowledge_service
        .repo
        .delete_source(&workspace_id, &source_id)
        .await
        .map_err(|e| e.to_string())
}

/// 搜索知识库（混合检索）
#[tauri::command]
pub async fn search_knowledge(
    workspace_id: String,
    query: String,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResultOutput>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }

    let results = state
        .knowledge_service
        .retriever
        .search(&workspace_id, &query, top_k.unwrap_or(5))
        .await
        .map_err(|e| e.to_string())?;

    Ok(results
        .into_iter()
        .map(|r| SearchResultOutput {
            content: r.content,
            source_title: r.metadata_json.as_deref()
                .and_then(|j| serde_json::from_str::<ChunkMetadata>(j).ok())
                .and_then(|m| m.source_title)
                .unwrap_or_default(),
            heading_path: r.heading_path,
            relevance_score: r.rrf_score,
        })
        .collect())
}
```

Tauri Command 注册（`main.rs`）：

```rust
// apps/desktop/src-tauri/src/main.rs

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // ... 其他 commands ...
            commands::knowledge::list_sources,
            commands::knowledge::add_source,
            commands::knowledge::remove_source,
            commands::knowledge::search_knowledge,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

---

## 11. 前端组件

### 11.1 组件层级

```text
KnowledgePanel
├── SourceList               ← 已导入文档列表
│   └── SourceItem           ← 单条文档（标题、类型、分块数、状态）
├── ImportDialog             ← 文件选择 / URL 输入对话框
├── SearchPreview            ← 知识库搜索预览
│   └── ChunkPreviewCard     ← 搜索结果卡片
└── ReindexButton            ← 手动重新索引按钮
```

### 11.2 KnowledgePanel 实现

```typescript
// apps/desktop/src/components/knowledge/KnowledgePanel.tsx

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Plus, Trash2, Search, FileText, Globe, Code, RefreshCw } from "lucide-react";

interface SourceSummary {
  id: string;
  title: string;
  source_type: string;
  chunk_count: number;
  status: string;
  created_at: number;
}

interface SearchResultOutput {
  content: string;
  source_title: string;
  heading_path: string | null;
  relevance_score: number;
}

interface KnowledgePanelProps {
  workspaceId: string;
}

export function KnowledgePanel({ workspaceId }: KnowledgePanelProps) {
  const [sources, setSources] = useState<SourceSummary[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResultOutput[]>([]);
  const [importDialogOpen, setImportDialogOpen] = useState(false);
  const [urlInput, setUrlInput] = useState("");
  const [loading, setLoading] = useState(false);

  // 加载知识源列表
  const loadSources = useCallback(async () => {
    const result = await invoke<SourceSummary[]>("list_sources", { workspaceId });
    setSources(result);
  }, [workspaceId]);

  useEffect(() => {
    loadSources();
  }, [loadSources]);

  // 导入本地文件
  const handleImportFile = async () => {
    const selected = await open({
      multiple: true,
      filters: [
        { name: "文档", extensions: ["md", "pdf", "txt"] },
        { name: "代码", extensions: ["rs", "py", "ts", "tsx", "js", "jsx", "go"] },
      ],
    });

    if (!selected) return;
    setLoading(true);

    try {
      const paths = Array.isArray(selected) ? selected : [selected];
      for (const filePath of paths) {
        await invoke("add_source", { workspaceId, uri: filePath });
      }
      await loadSources();
    } finally {
      setLoading(false);
      setImportDialogOpen(false);
    }
  };

  // 导入 URL
  const handleImportUrl = async () => {
    if (!urlInput.trim()) return;
    setLoading(true);

    try {
      await invoke("add_source", {
        workspaceId,
        uri: urlInput.trim(),
        sourceType: "url",
      });
      setUrlInput("");
      await loadSources();
    } finally {
      setLoading(false);
      setImportDialogOpen(false);
    }
  };

  // 删除知识源
  const handleRemove = async (sourceId: string) => {
    await invoke("remove_source", { workspaceId, sourceId });
    setSources((prev) => prev.filter((s) => s.id !== sourceId));
  };

  // 搜索知识库
  const handleSearch = async () => {
    if (!searchQuery.trim()) {
      setSearchResults([]);
      return;
    }
    const results = await invoke<SearchResultOutput[]>("search_knowledge", {
      workspaceId,
      query: searchQuery,
      topK: 5,
    });
    setSearchResults(results);
  };

  const typeIcon = (type: string) => {
    switch (type) {
      case "url": return <Globe size={16} />;
      case "code": return <Code size={16} />;
      default: return <FileText size={16} />;
    }
  };

  return (
    <div className="flex flex-col gap-4 p-4">
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold">知识库</h2>
        <Dialog open={importDialogOpen} onOpenChange={setImportDialogOpen}>
          <DialogTrigger asChild>
            <Button size="sm" variant="outline">
              <Plus size={14} className="mr-1" /> 添加
            </Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>添加知识源</DialogTitle>
            </DialogHeader>
            <div className="flex flex-col gap-4">
              <Button onClick={handleImportFile} disabled={loading} variant="outline">
                <FileText size={16} className="mr-2" />
                选择本地文件
              </Button>
              <div className="flex gap-2">
                <Input
                  placeholder="输入 URL..."
                  value={urlInput}
                  onChange={(e) => setUrlInput(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleImportUrl()}
                />
                <Button onClick={handleImportUrl} disabled={loading || !urlInput.trim()}>
                  导入
                </Button>
              </div>
            </div>
          </DialogContent>
        </Dialog>
      </div>

      {/* 搜索预览 */}
      <div className="flex gap-2">
        <Input
          placeholder="搜索知识库..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSearch()}
        />
        <Button onClick={handleSearch} variant="ghost" size="icon">
          <Search size={16} />
        </Button>
      </div>

      {searchResults.length > 0 && (
        <div className="space-y-2 rounded-md border p-3">
          <p className="text-xs text-muted-foreground">
            找到 {searchResults.length} 条相关内容
          </p>
          {searchResults.map((r, idx) => (
            <div key={idx} className="rounded border-l-2 border-primary/50 pl-3 py-2 text-sm">
              {r.heading_path && (
                <p className="text-xs text-muted-foreground mb-1">{r.heading_path}</p>
              )}
              <p className="line-clamp-3">{r.content}</p>
              <p className="text-xs text-muted-foreground mt-1">
                来源：{r.source_title} | 相关度：{(r.relevance_score * 1000).toFixed(1)}
              </p>
            </div>
          ))}
        </div>
      )}

      {/* 知识源列表 */}
      <div className="space-y-1">
        {sources.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-8">
            暂无知识源，点击"添加"导入文档
          </p>
        ) : (
          sources.map((source) => (
            <div
              key={source.id}
              className="flex items-center gap-3 rounded-md px-3 py-2 hover:bg-muted/50"
            >
              <span className="text-muted-foreground">{typeIcon(source.source_type)}</span>
              <div className="flex-1 min-w-0">
                <p className="text-sm font-medium truncate">{source.title}</p>
                <p className="text-xs text-muted-foreground">
                  {source.chunk_count} 个分块
                </p>
              </div>
              <Badge
                variant={source.status === "ready" ? "secondary" : "outline"}
                className="text-xs"
              >
                {source.status === "ready" ? source.source_type : source.status}
              </Badge>
              <Button
                variant="ghost"
                size="icon"
                className="h-7 w-7 text-muted-foreground hover:text-destructive"
                onClick={() => handleRemove(source.id)}
              >
                <Trash2 size={14} />
              </Button>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
```

---

## 12. 设计约束与性能目标

### 12.1 容量限制

| 维度 | 限制值 | 说明 |
| ---- | ------ | ---- |
| 每工作区知识源数量 | 100 个 | 超过后提示用户删除旧的知识源 |
| 单文档最大文件大小 | 10 MB | PDF / TXT / Markdown；代码文件 1 MB |
| 单文档最大分块数 | 500 个 | 超大文档截断，提示用户拆分 |
| 每工作区总分块数 | 50,000 个 | 超过后拒绝新增，需先删除 |
| 单块最大 Token 数 | 512 tokens | 递归分块器硬限 |
| 单块最小 Token 数 | 32 tokens | 低于此值与前块合并 |
| 重叠 Token 数 | 64 tokens | 相邻块间的上下文重叠 |
| 检索 Top-K 上限 | 20 | 工具和 Tauri Command 共同限制 |

### 12.2 性能目标

| 操作 | 目标延迟 | 实现手段 |
| ---- | -------- | -------- |
| 单文档导入（1MB Markdown） | < 10s | 分块 + 批量 embedding（32/batch） |
| 向量检索（5 万 chunk） | < 50ms | sqlite-vec 余弦距离线性扫描 |
| BM25 全文检索（5 万 chunk） | < 30ms | FTS5 内置索引 |
| 混合检索完整流程 | < 200ms | 向量 + BM25 并发 + RRF 融合 |
| 上下文注入格式化 | < 1ms | 纯字符串拼接 |
| 增量更新检测 | < 100ms | SHA-256 哈希比较 |

### 12.3 可靠性约束

- **事务一致性**：分块写入与知识源状态更新在同一事务内完成，任一步骤失败全部回滚
- **导入幂等性**：相同 `content_hash` 的文档不会重复导入，提示"文档未变更"
- **降级策略**：OpenAI Embedding API 不可用时自动降级到本地 ONNX 模型；两者都不可用时仅启用 BM25 全文检索（无向量检索）
- **工作区隔离**：所有查询 SQL 必须携带 `workspace_id` 条件，防止跨工作区数据泄露
- **级联删除**：删除知识源时通过 `ON DELETE CASCADE` 自动清理关联分块，FTS5 同步触发器自动更新全文索引

---

## 13. 目录结构

```
crates/agent-core/src/knowledge/
├── mod.rs                    # 模块导出
├── models.rs                 # KnowledgeSource / KnowledgeChunk 数据结构
├── service.rs                # KnowledgeService 业务编排
├── chunker.rs                # RecursiveChunker 分块算法
├── ingestor.rs               # KnowledgeIngestor trait + IngestorRegistry
├── ingestors/
│   ├── markdown.rs           # Markdown 解析器
│   ├── pdf.rs                # PDF 解析器
│   ├── text.rs               # 纯文本解析器
│   ├── url.rs                # URL / 网页解析器
│   └── code.rs               # 代码文件解析器（tree-sitter）
├── embedding.rs              # EmbeddingClient trait + 提供者选择
├── embedding_openai.rs       # OpenAI Embedding 实现
├── embedding_onnx.rs         # 本地 ONNX Embedding 实现
├── retriever.rs              # HybridRetriever（向量 + BM25 + RRF）
├── injector.rs               # 上下文注入格式化
├── sync.rs                   # KnowledgeSyncManager 增量更新
├── watcher.rs                # 文件变更监听
├── repo.rs                   # KnowledgeRepo（Repository 模式）
└── tests/
    ├── chunker_test.rs
    ├── retriever_test.rs
    └── ingestor_test.rs

crates/agent-core/src/tools/
└── knowledge_manage.rs       # knowledge_manage 工具实现

apps/desktop/src-tauri/src/commands/
└── knowledge.rs              # Tauri Commands

apps/desktop/src/components/knowledge/
└── KnowledgePanel.tsx        # 前端知识库管理面板

migrations/
└── 0009_knowledge_base.sql   # 数据库迁移
```

---

## 14. 相关文档

- [08-知识库RAG.md](../../03-系统设计阶段/02-核心功能模块/08-知识库RAG.md) — 系统设计：整体架构、6 种分块策略总览、KnowledgeBase 主实现、前端页面
- [01-记忆系统详细设计.md](01-记忆系统详细设计.md) — L2 语义记忆使用相同的 sqlite-vec + FTS5 检索模式，Embedding Provider 共享
- [01-数据库访问层详细设计.md](../06-安全与基础设施/01-数据库访问层详细设计.md) — Repository 模式规范、连接池初始化、sqlite-vec 加载、事务管理
- [04-全局搜索系统设计.md](../05-桌面端与交互/04-全局搜索系统设计.md) — 知识库作为全局搜索的搜索源之一，共享 `SearchSource` trait
- [07-上下文管理.md](../../03-系统设计阶段/02-核心功能模块/07-上下文管理.md) — Token 预算公式、上下文槽位分配
- [01-持久化层.md](../../03-系统设计阶段/03-基础设施/01-持久化层.md) — SQLite WAL 模式、加密配置
