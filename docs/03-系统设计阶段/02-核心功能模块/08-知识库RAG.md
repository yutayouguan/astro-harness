# 知识库（RAG）

> 阶段：系统设计 | 状态：定稿 | 说明：文档摄入、混合检索（BM25+sqlite-vec）

## 概述

知识库（RAG，Retrieval-Augmented Generation）使 Agent 能够检索用户上传的文档，将相关片段注入上下文，从而基于私有知识回答问题。每个工作区拥有独立知识库，数据完全隔离。

---

## 1. 整体架构

```
文档摄入层
  PDF / Markdown / TXT / 网页 URL / 代码文件
         │
         ▼
    文档解析 & 分块
  （段落 / Token / 滑动窗口 / 语义 / 父子 / 命题）
         │
         ▼
     索引层（双引擎）
  ┌─────────────────────────────────┐
  │  向量索引（sqlite-vec）          │
  │  全文索引（SQLite FTS5 / BM25）  │
  └─────────────────────────────────┘
         │
         ▼
     检索层（混合检索 + RRF 融合）
         │
         ▼
   注入上下文（System Prompt knowledge 区域）
```

---

## 2. 支持文档格式

| 格式 | 解析方式 | 备注 |
| ---- | ---- | ---- |
| PDF | `pdf-extract` crate | 提取文本层，不支持扫描版 |
| Markdown | 直接读取 | 保留标题层级信息辅助分块 |
| TXT | 直接读取 | 按行/段落分块 |
| 网页 URL | `reqwest` + `scraper` | 提取正文，去除导航/广告 |
| 代码文件 | `tree-sitter` 解析 | 按函数/类分块，保留语言标注 |

---

## 3. 数据库 Schema

```sql
-- 文档表（权威 Schema 见 01-Schema设计.md）
CREATE TABLE knowledge_sources (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    source_type  TEXT NOT NULL DEFAULT 'file',  -- 'file' | 'url' | 'text'
    uri          TEXT,                           -- 原始文件路径或 URL
    content_hash TEXT,                           -- SHA-256，用于重复检测
    mime_type    TEXT,
    file_size    INTEGER,
    chunk_count  INTEGER NOT NULL DEFAULT 0,
    status       TEXT NOT NULL DEFAULT 'processing' CHECK(status IN ('processing','ready','failed')),
    error_msg    TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

-- 文档分块表（向量存储在共享 embeddings vec0 表，source_type='knowledge'）
CREATE TABLE knowledge_chunks (
    id           TEXT PRIMARY KEY,
    source_id  TEXT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    chunk_index  INTEGER NOT NULL,               -- 块在文档中的顺序
    content      TEXT NOT NULL,
    token_count  INTEGER NOT NULL,               -- 真实 token 数（非字节数）
    metadata_json TEXT DEFAULT '{}',              -- JSON: { page, section, parent_chunk_id }
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

-- FTS5 全文索引（jieba 中文分词）
CREATE VIRTUAL TABLE chunks_fts USING fts5(
    content,
    content='knowledge_chunks',
    content_rowid='rowid',
    tokenize='unicode61'
);

-- 触发器：保持 FTS 与主表同步
CREATE TRIGGER chunks_fts_insert AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER chunks_fts_delete AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER chunks_fts_update AFTER UPDATE ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO chunks_fts(rowid, content) VALUES (new.rowid, new.content);
END;
```

---

## 4. 文档分块策略

### 4.1 策略总览

| 策略 | 适用场景 | 构建成本 | 检索质量 |
| ---- | -------- | -------- | -------- |
| `ByParagraph` | 通用文档、快速摄入 | 低 | 中 |
| `ByTokenCount` | 长文、需严格控制 Token 数 | 低 | 中 |
| `SlidingWindow` | 内容密度高、上下文强依赖 | 低 | 中+ |
| `Semantic` | 高质量语料、语义内聚要求高 | 中（需 embedding） | 高 |
| `ParentChild` | 长文精准问答、需保留上下文 | 中 | 高 |
| `Proposition` | 最高质量知识库、离线慢速构建 | 高（需 LLM 调用） | 最高 |

代码文件统一使用 `tree-sitter` 按函数/类边界切分，不走上述枚举。

### 分块策略自动选择（默认模式）

用户无需手动选择分块策略。系统根据文档类型自动匹配最佳策略：

| 文档类型 | 自动策略 | 理由 |
| ---- | ---- | ---- |
| Markdown (.md) | ParentChild (parent=512, child=128) | 自然段落边界清晰，父子结构保留上下文 |
| 代码文件 (.py/.rs/.ts/.js) | tree-sitter 函数/类边界 | 保持代码语义完整性 |
| PDF | SlidingWindow (512 tokens, 128 overlap) | PDF 缺乏结构化标记 |
| 纯文本 (.txt) | ByTokenCount (256 tokens) | 无结构可利用 |
| 长文档 (> 50 页) | Semantic (阈值 0.75) | 语义边界比固定窗口更精准 |

专家用户可在知识库设置中切换为"手动模式"，选择全部 6 种策略及参数。默认自动模式覆盖 90% 场景。

### 文档增量更新

当用户修改已上传的文档后，调用 `update_document` 方法重新处理：

```rust
pub async fn update_document(&self, doc_id: &str, new_content: &[u8]) -> Result<()> {
    let tx = self.pool.begin().await?;
    // 1. 删除旧 chunks 的 embeddings（共享 embeddings 表）
    sqlx::query("DELETE FROM embeddings WHERE source_id IN (SELECT id FROM knowledge_chunks WHERE source_id = ?) AND source_type = 'knowledge'")
        .bind(doc_id).execute(&mut *tx).await?;
    // 2. 删除旧 chunks
    sqlx::query("DELETE FROM knowledge_chunks WHERE source_id = ?")
        .bind(doc_id).execute(&mut *tx).await?;
    // 3. 重新分块 + 向量化（复用 ingest 逻辑）
    let chunks = self.chunk(new_content, &self.auto_strategy(doc_id)).await?;
    self.store_chunks(&mut tx, doc_id, &chunks).await?;
    tx.commit().await?;
    Ok(())
}
```

### 4.2 Rust 实现

```rust
// crates/agent-core/src/knowledge/chunker.rs

#[derive(Debug, Clone)]
pub enum ChunkStrategy {
    /// 按段落分块（空行分隔）
    ByParagraph { max_tokens: usize },
    /// 按固定 Token 数分块
    ByTokenCount { chunk_size: usize, overlap: usize },
    /// 滑动窗口（Token 数 + 重叠）
    SlidingWindow { window: usize, stride: usize },
    /// 语义分块：相邻句子 embedding 余弦相似度骤降处切分
    Semantic { threshold: f32, max_tokens: usize },
    /// 父子分块：大块存上下文，小块用于检索
    ParentChild { child_size: usize, parent_size: usize, overlap: usize },
    /// 命题分块：LLM 将段落改写为原子事实句后再分块
    Proposition { max_tokens: usize },
}

pub struct Chunker {
    strategy: ChunkStrategy,
    tokenizer: tiktoken_rs::CoreBPE,
}

impl Chunker {
    pub async fn chunk(&self, text: &str, metadata: serde_json::Value) -> Result<Vec<Chunk>> {
        match &self.strategy {
            ChunkStrategy::ByParagraph { max_tokens } => {
                Ok(self.chunk_by_paragraph(text, *max_tokens, metadata))
            }
            ChunkStrategy::ByTokenCount { chunk_size, overlap } => {
                Ok(self.chunk_by_tokens(text, *chunk_size, *overlap, metadata))
            }
            ChunkStrategy::SlidingWindow { window, stride } => {
                Ok(self.sliding_window(text, *window, *stride, metadata))
            }
            ChunkStrategy::Semantic { threshold, max_tokens } => {
                // 异步方法：需要 embedder 进行语义边界检测
                self.chunk_by_semantic(text, *threshold, *max_tokens, metadata).await
            }
            ChunkStrategy::ParentChild { child_size, parent_size, overlap } => {
                // 实现见 4.4；返回小块，父块路径存入 metadata
                Ok(self.chunk_parent_child(text, *child_size, *parent_size, *overlap, metadata))
            }
            ChunkStrategy::Proposition { max_tokens: _ } => {
                // 异步方法：需要 LLM 将文本改写为命题
                self.rewrite_to_propositions(text, metadata).await
            }
        }
    }

    fn chunk_by_paragraph(&self, text: &str, max_tokens: usize, meta: serde_json::Value) -> Vec<Chunk> {
        let paragraphs: Vec<&str> = text.split("\n\n").filter(|s| !s.trim().is_empty()).collect();
        let mut chunks = vec![];
        let mut buf = String::new();
        let mut buf_tokens = 0;

        for para in paragraphs {
            let para_tokens = self.tokenizer.encode_with_special_tokens(para).len();
            if buf_tokens + para_tokens > max_tokens && !buf.is_empty() {
                chunks.push(Chunk { content: buf.trim().to_string(), metadata: meta.clone(), parent_content: None });
                buf = String::new();
                buf_tokens = 0;
            }
            buf.push_str(para);
            buf.push_str("\n\n");
            buf_tokens += para_tokens;
        }
        if !buf.trim().is_empty() {
            chunks.push(Chunk { content: buf.trim().to_string(), metadata: meta, parent_content: None });
        }
        chunks
    }
}

#[derive(Debug)]
pub struct Chunk {
    pub content: String,
    pub metadata: serde_json::Value,
    /// 仅 ParentChild 策略填充：父块原文，检索命中后注入给 LLM
    pub parent_content: Option<String>,
}
```

### 4.3 语义分块（Semantic Chunking）

将文本按句子切分后，对相邻句子对计算 embedding 余弦相似度；相似度低于阈值（默认 0.75）处视为语义边界，合并同一语义段内的所有句子为一块。

```rust
// crates/agent-core/src/knowledge/chunker.rs

impl Chunker {
    /// 语义分块：需在异步上下文中调用 embedder
    pub async fn chunk_by_semantic(
        &self,
        text: &str,
        threshold: f32,
        max_tokens: usize,
        embedder: &dyn Embedder,
        meta: serde_json::Value,
    ) -> Result<Vec<Chunk>> {
        // 1. 按句子切分（。！？\n 等）
        let sentences: Vec<&str> = split_sentences(text);
        if sentences.is_empty() {
            return Ok(vec![]);
        }

        // 2. 批量 embed 所有句子
        let embeddings = embedder.embed_batch(sentences.clone()).await?;

        // 3. 计算相邻相似度，找切点
        let mut boundaries = vec![0usize];
        for i in 1..sentences.len() {
            let sim = cosine_similarity(&embeddings[i - 1], &embeddings[i]);
            if sim < threshold {
                boundaries.push(i);
            }
        }
        boundaries.push(sentences.len());

        // 4. 合并段内句子，超过 max_tokens 时强制再切
        let mut chunks = vec![];
        for window in boundaries.windows(2) {
            let (start, end) = (window[0], window[1]);
            let segment = sentences[start..end].join("");
            let tokens = self.tokenizer.encode_with_special_tokens(&segment).len();
            if tokens <= max_tokens {
                chunks.push(Chunk { content: segment, metadata: meta.clone(), parent_content: None });
            } else {
                // 段内 token 仍超限，降级为 ByTokenCount
                let sub = self.chunk_by_tokens(&segment, max_tokens, max_tokens / 10, meta.clone());
                chunks.extend(sub);
            }
        }
        Ok(chunks)
    }
}
```

### 4.4 父子分块（Parent-Child Chunking）

大块（parent，512–1024 token）保留完整上下文，小块（child，128–256 token）精准命中检索。检索时返回命中的小块 ID，注入给 LLM 时自动扩展为对应父块，兼顾检索精度与上下文完整性。

```rust
// crates/agent-core/src/knowledge/chunker.rs

impl Chunker {
    fn chunk_parent_child(
        &self,
        text: &str,
        child_size: usize,
        parent_size: usize,
        overlap: usize,
        meta: serde_json::Value,
    ) -> Vec<Chunk> {
        // 1. 先按 parent_size 切出父块
        let parents = self.chunk_by_tokens(text, parent_size, overlap, meta.clone());

        // 2. 每个父块内再按 child_size 切小块，parent_content 指向父块原文
        let mut chunks = vec![];
        for parent in &parents {
            let children = self.chunk_by_tokens(&parent.content, child_size, overlap / 2, meta.clone());
            for mut child in children {
                child.parent_content = Some(parent.content.clone());
                chunks.push(child);
            }
        }
        chunks
    }
}
```

数据库层面，`knowledge_chunks` 表新增 `parent_content TEXT` 列存储父块原文；检索时 `SELECT parent_content` 即可直接获取，无需二次查询。

### 4.5 命题分块（Proposition Chunking）

通过 LLM 将段落改写为若干**独立、自包含的原子事实句**（proposition），每句即一个块。适合高质量专业知识库，查询与命题的语义对齐度最高，但需额外 LLM 调用，构建较慢，建议离线批量处理。

```rust
// crates/agent-core/src/knowledge/chunker.rs

impl Chunker {
    /// 命题分块：调用 LLM 将段落分解为原子事实句
    pub async fn rewrite_to_propositions(
        &self,
        text: &str,
        llm: &dyn TextClient,
        meta: serde_json::Value,
    ) -> Result<Vec<Chunk>> {
        let prompt = format!(
            "将以下段落分解为独立、自包含的原子事实句列表，每行一句，不要编号，不要解释：\n\n{}",
            text
        );

        let response = llm.complete(&prompt, Default::default()).await?;

        let propositions: Vec<Chunk> = response
            .lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(|line| Chunk {
                content: line.to_string(),
                metadata: meta.clone(),
                parent_content: Some(text.to_string()), // 保留原段落供上下文扩展
            })
            .collect();

        Ok(propositions)
    }
}
```

**摄入流程差异：** `Proposition` 策略在 `KnowledgeBase::ingest` 中需先调用 `rewrite_to_propositions`，再对返回的命题列表做 embedding；其他策略直接走 `chunker.chunk()` 同步路径。

---

## 5. Rust KnowledgeBase 实现

```rust
// crates/agent-core/src/knowledge/mod.rs

use anyhow::Result;
use uuid::Uuid;

pub struct KnowledgeBase {
    db: sqlx::SqlitePool,
    embedder: Box<dyn Embedder>,
    chunker: Chunker,
}

impl KnowledgeBase {
    /// 摄入文档：解析 → 分块 → 向量化 → 存储
    pub async fn ingest(&self, workspace_id: &str, source: DocumentSource) -> Result<String> {
        let doc_id = Uuid::new_v4().to_string();
        let text = source.extract_text().await?;

        // 重复检测：基于内容 SHA-256 hash
        let content_hash = sha256(text.as_bytes());
        let existing = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM knowledge_sources WHERE workspace_id = ? AND content_hash = ?"
        )
        .bind(workspace_id).bind(&content_hash)
        .fetch_one(&self.db).await?;

        if existing > 0 {
            return Err(RagError::DuplicateDocument {
                hash: content_hash,
                message: "该文档已存在于知识库中，如需更新请使用 update_document".into(),
            });
        }

        let chunks = self.chunker.chunk(&text, source.metadata()).await?;

        let mut tx = self.db.begin().await?;

        // 插入文档记录
        sqlx::query(
            "INSERT INTO knowledge_sources (id, workspace_id, title, source_type, uri, content_hash, chunk_count, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'ready')"
        )
        .bind(&doc_id).bind(workspace_id).bind(source.title())
        .bind(source.source_type()).bind(source.uri())
        .bind(&content_hash).bind(chunks.len() as i64)
        .execute(&mut *tx)
        .await?;

        // 插入分块 + 写入向量到共享 embeddings 表
        for (i, chunk) in chunks.iter().enumerate() {
            let chunk_id = Uuid::new_v4().to_string();

            // 写入 chunk
            sqlx::query(
                "INSERT INTO knowledge_chunks (id, source_id, workspace_id, chunk_index, content, token_count, metadata_json)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            )
            .bind(&chunk_id).bind(&doc_id).bind(workspace_id)
            .bind(i as i32).bind(&chunk.content)
            .bind(self.count_tokens(&chunk.content))  // 使用 tiktoken 计算真实 token 数
            .bind(&serde_json::to_string(&chunk.metadata)?)
            .execute(&mut *tx).await?;

            // 写入向量到共享 embeddings 表
            let embedding = self.embedder.embed(&chunk.content).await?;
            sqlx::query(
                "INSERT INTO embeddings (source_id, source_type, workspace_id, embedding)
                 VALUES (?1, 'knowledge', ?2, ?3)"
            )
            .bind(&chunk_id).bind(workspace_id).bind(&embedding)
            .execute(&mut *tx).await?;
        }

        tx.commit().await?;
        Ok(doc_id)
    }

    /// 使用 tiktoken 计算真实 token 数（非 UTF-8 字节数）
    /// 注意：`.len()` 返回 UTF-8 字节数，中文文本误差达 1.5-4 倍。
    /// 使用 `tiktoken_rs::CoreBPE` 计算准确的 token 数。
    fn count_tokens(&self, text: &str) -> i64 {
        self.chunker.tokenizer.encode_with_special_tokens(text).len() as i64
    }

    /// 混合检索：向量 + BM25 + RRF 融合
    pub async fn search(
        &self,
        workspace_id: &str,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<SearchResult>> {
        // 1. 向量检索
        let query_embedding = self.embedder.embed(query).await?;
        let vector_results = self.vector_search(workspace_id, &query_embedding, top_k * 3).await?;

        // 2. BM25 全文检索
        let fts_results = self.fts_search(workspace_id, query, top_k * 3).await?;

        // 3. RRF（Reciprocal Rank Fusion）融合排序
        let fused = rrf_fusion(vector_results, fts_results, 60.0);

        // 过滤低相关性结果，防止不相关内容注入 LLM
        const MIN_RELEVANCE_SCORE: f32 = 0.005;
        let filtered: Vec<_> = fused.into_iter()
            .filter(|r| r.rrf_score >= MIN_RELEVANCE_SCORE)
            .take(top_k)
            .collect();

        if filtered.is_empty() {
            tracing::debug!("知识库检索无相关结果（所有结果分数低于 {}）", MIN_RELEVANCE_SCORE);
        }

        Ok(filtered)
    }

    /// 删除文档及其所有分块
    pub async fn delete_doc(&self, workspace_id: &str, doc_id: &str) -> Result<()> {
        sqlx::query!(
            "DELETE FROM knowledge_sources WHERE id = ? AND workspace_id = ?",
            doc_id, workspace_id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn vector_search(
        &self,
        workspace_id: &str,
        embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<RankedChunk>> {
        // 使用共享 embeddings vec0 表的 MATCH 语法进行 KNN 检索
        let rows = sqlx::query_as::<_, ChunkMatch>(
            "SELECT e.source_id, e.distance, dc.content, dc.metadata_json, d.title, d.uri
             FROM embeddings e
             JOIN knowledge_chunks dc ON e.source_id = dc.id
             JOIN knowledge_sources d ON dc.source_id = d.id
             WHERE e.embedding MATCH ?1
               AND e.workspace_id = ?2
               AND e.source_type = 'knowledge'
               AND e.k = ?3
             ORDER BY e.distance"
        )
        .bind(embedding)
        .bind(workspace_id)
        .bind(limit as i32)
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().enumerate().map(|(rank, r)| RankedChunk {
            id: r.source_id,
            content: r.content,
            metadata: r.metadata_json.unwrap_or_default(),
            rank,
            score: 1.0 - r.distance as f32,
        }).collect())
    }

    async fn fts_search(
        &self,
        workspace_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<RankedChunk>> {
        let rows = sqlx::query!(
            r#"
            SELECT dc.id, dc.content, dc.metadata_json,
                   bm25(chunks_fts) as bm25_score
            FROM chunks_fts
            JOIN knowledge_chunks dc ON dc.rowid = chunks_fts.rowid
            WHERE chunks_fts MATCH ? AND dc.workspace_id = ?
            ORDER BY bm25_score
            LIMIT ?
            "#,
            query, workspace_id, limit as i64
        )
        .fetch_all(&self.db)
        .await?;

        Ok(rows.into_iter().enumerate().map(|(rank, r)| RankedChunk {
            id: r.id,
            content: r.content,
            metadata: r.metadata_json.unwrap_or_default(),
            rank,
            score: r.bm25_score.unwrap_or(0.0) as f32,
        }).collect())
    }
}

/// RRF 融合排序（Reciprocal Rank Fusion）
fn rrf_fusion(
    vector_results: Vec<RankedChunk>,
    fts_results: Vec<RankedChunk>,
    k: f32,
) -> Vec<SearchResult> {
    use std::collections::HashMap;

    let mut scores: HashMap<String, f32> = HashMap::new();
    let mut content_map: HashMap<String, (String, String)> = HashMap::new();

    for chunk in &vector_results {
        *scores.entry(chunk.id.clone()).or_default() += 1.0 / (k + chunk.rank as f32 + 1.0);
        content_map.entry(chunk.id.clone()).or_insert_with(|| (chunk.content.clone(), chunk.metadata.clone()));
    }
    for chunk in &fts_results {
        *scores.entry(chunk.id.clone()).or_default() += 1.0 / (k + chunk.rank as f32 + 1.0);
        content_map.entry(chunk.id.clone()).or_insert_with(|| (chunk.content.clone(), chunk.metadata.clone()));
    }

    let mut results: Vec<SearchResult> = scores
        .into_iter()
        .map(|(id, score)| {
            let (content, metadata) = content_map.remove(&id).unwrap_or_default();
            SearchResult { chunk_id: id, content, metadata, rrf_score: score }
        })
        .collect();

    results.sort_by(|a, b| b.rrf_score.partial_cmp(&a.rrf_score).unwrap());
    results
}

#[derive(Debug)]
pub struct RankedChunk {
    pub id: String,
    pub content: String,
    pub metadata: String,
    pub rank: usize,
    pub score: f32,
}

#[derive(Debug)]
pub struct SearchResult {
    pub chunk_id: String,
    pub content: String,
    pub metadata: String,
    pub rrf_score: f32,
}
```

---

## 6. Tauri Commands

```rust
// src-tauri/src/commands/knowledge.rs

use tauri::State;
use crate::AppState;

#[tauri::command]
pub async fn upload_document(
    workspace_id: String,
    file_path: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let source = DocumentSource::from_file(&file_path)
        .map_err(|e| e.to_string())?;
    state.knowledge_base
        .ingest(&workspace_id, source)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_documents(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<DocumentInfo>, String> {
    sqlx::query_as!(
        DocumentInfo,
        "SELECT id, title, source_type, file_size, chunk_count, created_at
         FROM knowledge_sources WHERE workspace_id = ? ORDER BY created_at DESC",
        workspace_id
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_knowledge(
    workspace_id: String,
    query: String,
    top_k: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SearchResult>, String> {
    state.knowledge_base
        .search(&workspace_id, &query, top_k.unwrap_or(5))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_document(
    workspace_id: String,
    doc_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state.knowledge_base
        .delete_doc(&workspace_id, &doc_id)
        .await
        .map_err(|e| e.to_string())
}
```

---

## 7. 前端知识库管理页面

```tsx
// src/pages/KnowledgeBasePage.tsx

import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDropzone } from "react-dropzone";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Trash2, Search, Upload, FileText } from "lucide-react";

interface DocumentInfo {
  id: string;
  title: string;
  source_type: string;
  file_size: number;
  chunk_count: number;
  created_at: string;
}

interface SearchResult {
  chunk_id: string;
  content: string;
  metadata: string;
  rrf_score: number;
}

export function KnowledgeBasePage({ workspaceId }: { workspaceId: string }) {
  const [documents, setDocuments] = useState<DocumentInfo[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<SearchResult[]>([]);
  const [uploading, setUploading] = useState(false);

  const onDrop = useCallback(async (acceptedFiles: File[]) => {
    setUploading(true);
    try {
      for (const file of acceptedFiles) {
        await invoke("upload_document", {
          workspaceId,
          filePath: file.path, // Tauri 提供真实路径
        });
      }
      // 刷新文档列表
      const docs = await invoke<DocumentInfo[]>("list_documents", { workspaceId });
      setDocuments(docs);
    } finally {
      setUploading(false);
    }
  }, [workspaceId]);

  const { getRootProps, getInputProps, isDragActive } = useDropzone({
    onDrop,
    accept: {
      "application/pdf": [".pdf"],
      "text/markdown": [".md"],
      "text/plain": [".txt"],
    },
  });

  const handleSearch = async () => {
    if (!searchQuery.trim()) return;
    const results = await invoke<SearchResult[]>("search_knowledge", {
      workspaceId,
      query: searchQuery,
      topK: 5,
    });
    setSearchResults(results);
  };

  const handleDelete = async (docId: string) => {
    await invoke("delete_document", { workspaceId, docId });
    setDocuments((prev) => prev.filter((d) => d.id !== docId));
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <h1 className="text-2xl font-bold">知识库</h1>

      {/* 上传区 */}
      <div
        {...getRootProps()}
        className={`border-2 border-dashed rounded-lg p-8 text-center cursor-pointer transition-colors ${
          isDragActive ? "border-primary bg-primary/5" : "border-muted-foreground/30"
        }`}
      >
        <input {...getInputProps()} />
        <Upload className="mx-auto mb-2 text-muted-foreground" size={32} />
        <p className="text-muted-foreground">
          {uploading ? "上传中..." : "拖拽文件到此处，或点击选择（支持 PDF、Markdown、TXT）"}
        </p>
      </div>

      {/* 搜索预览 */}
      <div className="flex gap-2">
        <Input
          placeholder="搜索知识库..."
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSearch()}
        />
        <Button onClick={handleSearch} variant="outline">
          <Search size={16} />
        </Button>
      </div>

      {searchResults.length > 0 && (
        <div className="space-y-2">
          <p className="text-sm text-muted-foreground">检索结果（{searchResults.length} 条）</p>
          {searchResults.map((r) => (
            <div key={r.chunk_id} className="rounded-md border p-3 text-sm">
              <p className="line-clamp-3">{r.content}</p>
              <Badge variant="outline" className="mt-1 text-xs">
                相关度 {(r.rrf_score * 100).toFixed(1)}
              </Badge>
            </div>
          ))}
        </div>
      )}

      {/* 文档列表 */}
      <div className="space-y-2">
        <p className="text-sm font-medium">已上传文档（{documents.length}）</p>
        {documents.map((doc) => (
          <div key={doc.id} className="flex items-center gap-3 rounded-md border p-3">
            <FileText size={20} className="shrink-0 text-muted-foreground" />
            <div className="flex-1 min-w-0">
              <p className="font-medium truncate">{doc.title}</p>
              <p className="text-xs text-muted-foreground">
                {doc.chunk_count} 个分块 · {(doc.file_size / 1024).toFixed(1)} KB
              </p>
            </div>
            <Badge variant="secondary">{doc.source_type}</Badge>
            <Button
              variant="ghost"
              size="icon"
              onClick={() => handleDelete(doc.id)}
              className="shrink-0 text-destructive"
            >
              <Trash2 size={16} />
            </Button>
          </div>
        ))}
      </div>
    </div>
  );
}
```

---

### RAG 上下文注入格式

检索结果作为独立的 `<knowledge>` 块注入 System Prompt，与 `<memory>` 块平级，不嵌套在 Slot [4] 内部：

```xml
<knowledge>
  <chunk index="1" source="设计文档.pdf" heading="# 架构 > ## 分层设计">
    检索到的文本内容...
  </chunk>
  <chunk index="2" source="API手册.md" heading="# API 设计 > ## 认证">
    另一段检索到的内容...
  </chunk>
</knowledge>
```

**Token 预算**：RAG 上下文最多占用 2,000 tokens。超出时按 relevance 分数降序截断。

**空结果处理**：当检索结果全部低于 `MIN_RELEVANCE_SCORE`（0.005）时，不注入任何 RAG 上下文，避免不相关内容误导 LLM。

**来源引用**：每个 chunk 的 `source` 和 `page/section` 属性帮助 LLM 在回答中注明出处（如 "根据《设计文档.pdf》第12页..."）。

---

## 8. 目录结构

```
crates/agent-core/src/knowledge/
├── mod.rs                  # KnowledgeBase 主结构体
├── chunker.rs              # 文档分块策略
├── embedder.rs             # Embedding trait + 实现
├── source.rs               # DocumentSource（PDF/Markdown/URL/代码）
├── search.rs               # 向量检索 + FTS + RRF 融合
└── tests/
    ├── ingest_test.rs
    └── search_test.rs

src/pages/
└── KnowledgeBasePage.tsx   # 前端知识库管理页面
```

---

## 相关文档

- [01-持久化层.md](../03-基础设施/01-持久化层.md) — SQLite 持久化层
- [05-多工作区隔离.md](../06-桌面端/05-多工作区隔离.md) — 多工作区隔离
- [07-上下文管理.md](07-上下文管理.md) — 检索结果注入上下文
