# Astro Agent SQLite Schema 设计

> 阶段：系统设计 | 状态：定稿 | 说明：21 表 + 3 虚拟表 + 1 视图 DDL（权威）

> **当前 Harness 基线（2026-08-29）**：本文的“单一 `agent.db`”和表清单是历史方案，不是当前运行时事实源。当前 Harness 使用 rollout 事件源，并由 `{base}/data/` 中的 `state.db`、`subagents-v2.db`、`usage.db`、`artifacts.db`、`knowledge.db`、`cron_v1.db` 等职责数据库形成投影。当前字段与迁移以各 crate schema 和测试为准。

## 一、总体说明

**历史 DB 架构决策：单一全局 `agent.db`**

全部工作区数据存入 `~/.astro/agent.db` 这一个 SQLite 文件，通过 `workspace_id` 外键实现逻辑隔离。不采用"每工作区独立 SQLite"方案，原因：跨工作区聚合查询（成本汇总、全局搜索、Skill 统计）是 P1 需求；独立文件方案需运行时 `ATTACH DATABASE` 拼接，迁移管理复杂度高。知识库向量索引、媒体文件、AI 产物仍物理隔离于各工作区目录。

数据库采用 WAL 模式与外键约束。所有主键统一使用 TEXT 类型的 UUID，时间字段统一使用 INTEGER（Unix 毫秒时间戳）。

---

## 二、完整 CREATE TABLE SQL

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- 1. 工作区配置表
CREATE TABLE IF NOT EXISTS workspaces (
    id          TEXT PRIMARY KEY,                        -- UUID
    name        TEXT NOT NULL,                           -- 工作区名称
    root_path   TEXT NOT NULL,                           -- 本地根目录路径
    config_json TEXT NOT NULL DEFAULT '{}',              -- 扩展配置（JSON）
    is_active   INTEGER NOT NULL DEFAULT 1,              -- 1=活跃 0=归档
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at  INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

-- 2. 对话会话表
CREATE TABLE IF NOT EXISTS conversations (
    id                      TEXT PRIMARY KEY,
    workspace_id            TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title                   TEXT NOT NULL DEFAULT '新对话',
    model                   TEXT NOT NULL,                         -- 使用的模型 ID
    system_prompt           TEXT,                                  -- 会话级系统提示
    token_input             INTEGER NOT NULL DEFAULT 0,            -- 累计输入 token
    token_output            INTEGER NOT NULL DEFAULT 0,            -- 累计输出 token
    status                  TEXT NOT NULL DEFAULT 'active'         -- active/archived/deleted
                            CHECK(status IN ('active','archived','deleted')),
    is_pinned               INTEGER NOT NULL DEFAULT 0,            -- 1=置顶（F-18）
    parent_conversation_id  TEXT REFERENCES conversations(id) ON DELETE SET NULL, -- 分支来源（F-12）
    branch_point_message_id TEXT REFERENCES messages(id) ON DELETE SET NULL,      -- 分支起始消息
    created_at              INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at              INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_conv_pinned ON conversations(workspace_id, is_pinned, updated_at);
CREATE INDEX IF NOT EXISTS idx_conv_branch ON conversations(parent_conversation_id) WHERE parent_conversation_id IS NOT NULL;

-- 3. 消息表
CREATE TABLE IF NOT EXISTS messages (
    id                TEXT PRIMARY KEY,
    conversation_id   TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    role              TEXT NOT NULL CHECK(role IN ('user','assistant','tool','system')),
    content           TEXT NOT NULL,                     -- 文本内容或 JSON（多模态）
    reasoning_content TEXT,                              -- DeepSeek / MiniMax 思考内容
    tool_call_id      TEXT,                              -- tool 调用关联 ID
    tool_name         TEXT,                              -- 调用的工具名称
    token_count       INTEGER NOT NULL DEFAULT 0,
    latency_ms        INTEGER,                           -- 响应延迟（毫秒）
    is_pinned         INTEGER NOT NULL DEFAULT 0,        -- 1=上下文压缩时保留（F-14）
    created_at        INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_messages_conv ON messages(conversation_id, created_at);
CREATE INDEX IF NOT EXISTS idx_messages_pinned ON messages(conversation_id, is_pinned) WHERE is_pinned = 1;

-- 4. 记忆条目表（支持时间版本管理，旧版本软删除不物理移除）
CREATE TABLE IF NOT EXISTS memory_entries (
    id                TEXT PRIMARY KEY,
    workspace_id      TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    category          TEXT NOT NULL DEFAULT 'general',   -- daily_event/case_lesson/work_pattern/core_insight
    key               TEXT NOT NULL,                     -- 记忆键名
    value             TEXT NOT NULL,                     -- 记忆内容
    source            TEXT,                              -- 来源（conversation_id 等）
    importance        REAL NOT NULL DEFAULT 0.5          -- 初始重要度 [0,1]
                      CHECK(importance BETWEEN 0.0 AND 1.0),
    half_life_days    INTEGER NOT NULL DEFAULT 30,       -- Weibull 衰减基础半衰期（天）
    retrieval_count   INTEGER NOT NULL DEFAULT 0,        -- 被 KNN 命中并实际使用的次数（影响有效半衰期）
    last_retrieved_at INTEGER,                           -- 最后检索时间戳（Unix ms）
    -- 时间版本管理（软替换，历史版本保留）
    valid_from        INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000), -- 本版本生效时间
    valid_to          INTEGER,                           -- 本版本失效时间（NULL = 当前活跃版本）
    superseded_by     TEXT REFERENCES memory_entries(id),-- 替换本版本的新条目 ID
    -- 其他生命周期字段
    expires_at        INTEGER,                           -- TTL 过期时间（NULL = 永不过期）
    distilled_at      INTEGER,                           -- 蒸馏至持久层的时间戳
    created_at        INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
-- 保证同一 key 只有一条活跃版本（valid_to IS NULL）
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_active_key
    ON memory_entries(workspace_id, category, key) WHERE valid_to IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_weibull
    ON memory_entries(workspace_id, category, retrieval_count, created_at) WHERE valid_to IS NULL;
CREATE INDEX IF NOT EXISTS idx_memory_expires
    ON memory_entries(expires_at) WHERE expires_at IS NOT NULL AND valid_to IS NULL;

-- 5. Skills 定义表
CREATE TABLE IF NOT EXISTS skills (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE, -- NULL=全局 Skill
    name         TEXT NOT NULL,
    description  TEXT NOT NULL,
    trigger_patterns TEXT DEFAULT '[]',                   -- JSON 数组，BM25 触发关键词
    entry_file   TEXT,                                   -- SKILL.md 路径
    version      TEXT NOT NULL DEFAULT '1.0.0',          -- semver 版本号
    content_hash TEXT NOT NULL DEFAULT '',               -- SHA256，content-addressable
    status       TEXT NOT NULL DEFAULT 'ready'           -- draft / ready / published / deprecated / evolving
                 CHECK(status IN ('draft','ready','published','deprecated','evolving')),
    origin       TEXT NOT NULL DEFAULT 'imported'        -- builtin / synthesized / imported / marketplace
                 CHECK(origin IN ('builtin','synthesized','imported','marketplace')),
    parent_id    TEXT REFERENCES skills(id),             -- 从哪个版本演化来
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    UNIQUE(workspace_id, name, version)
);

-- 6. Prompt 模板版本表
CREATE TABLE IF NOT EXISTS prompt_versions (
    id          TEXT PRIMARY KEY,
    skill_id    TEXT REFERENCES skills(id) ON DELETE SET NULL,
    name        TEXT NOT NULL,                           -- 模板名称
    content     TEXT NOT NULL,                           -- 模板正文（支持变量占位符）
    variables   TEXT NOT NULL DEFAULT '[]',              -- JSON 数组，变量声明
    version     INTEGER NOT NULL DEFAULT 1,              -- 递增版本号
    is_current  INTEGER NOT NULL DEFAULT 0,              -- 1=当前启用版本
    author      TEXT,
    change_note TEXT,                                    -- 本版变更说明
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_pv_skill ON prompt_versions(skill_id, version DESC);

-- 7. 成本统计表
CREATE TABLE IF NOT EXISTS cost_records (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL,
    task_id         TEXT REFERENCES task_traces(id) ON DELETE SET NULL,
    provider        TEXT NOT NULL,                       -- 如 "anthropic" / "openai" / "minimax"
    model           TEXT NOT NULL,
    input_tokens    INTEGER NOT NULL DEFAULT 0,
    output_tokens   INTEGER NOT NULL DEFAULT 0,
    cost_usd        REAL NOT NULL DEFAULT 0.0,           -- 折算美元成本
    recorded_at     INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_cost_ws_date     ON cost_records(workspace_id, recorded_at);
CREATE INDEX IF NOT EXISTS idx_cost_provider    ON cost_records(provider, recorded_at);

-- 7b. 预算配置表（运行时可通过 set_budget 命令修改）
CREATE TABLE IF NOT EXISTS budget_configs (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT REFERENCES workspaces(id) ON DELETE CASCADE, -- NULL = 全局配置
    scope           TEXT NOT NULL DEFAULT 'global'       -- 'global' / 'workspace' / 'conversation'
                    CHECK(scope IN ('global','workspace','conversation')),
    daily_limit_usd REAL,                               -- NULL = 无限制
    monthly_limit_usd REAL,
    warn_at_pct     INTEGER NOT NULL DEFAULT 80,         -- 达到上限百分比时警告
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

-- 7c. 成本日汇总表（非 VIEW，支持 INSERT...ON CONFLICT 快速查询）
-- 原设计为 VIEW，但 `BudgetManager::check_before_call()` 需要对其执行 INSERT...ON CONFLICT（物化缓存），SQLite 不支持对 VIEW 的 INSERT 操作。改为真正的 TABLE + AFTER INSERT 触发器自动维护。
CREATE TABLE IF NOT EXISTS daily_cost_summary (
    workspace_id TEXT NOT NULL,
    provider     TEXT NOT NULL,
    day          TEXT NOT NULL,     -- 'YYYY-MM-DD' 格式
    total_input  INTEGER NOT NULL DEFAULT 0,
    total_output INTEGER NOT NULL DEFAULT 0,
    total_cost   REAL NOT NULL DEFAULT 0.0,
    call_count   INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (workspace_id, provider, day)
);

-- 每次写入 cost_records 时自动更新汇总
CREATE TRIGGER IF NOT EXISTS cost_records_after_insert
AFTER INSERT ON cost_records
BEGIN
    INSERT INTO daily_cost_summary (workspace_id, provider, day, total_input, total_output, total_cost, call_count)
    VALUES (
        NEW.workspace_id,
        NEW.provider,
        strftime('%Y-%m-%d', NEW.recorded_at / 1000, 'unixepoch'),
        NEW.input_tokens,
        NEW.output_tokens,
        NEW.cost_usd,
        1
    )
    ON CONFLICT(workspace_id, provider, day) DO UPDATE SET
        total_input  = total_input + NEW.input_tokens,
        total_output = total_output + NEW.output_tokens,
        total_cost   = total_cost + NEW.cost_usd,
        call_count   = call_count + 1;
END;

-- 8. 审计日志表（工具调用专用字段内嵌，通用操作写入 detail_json）
CREATE TABLE IF NOT EXISTS audit_logs (
    id            TEXT PRIMARY KEY,
    workspace_id  TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
    session_id    TEXT,                                  -- 所属会话
    actor         TEXT NOT NULL DEFAULT 'system',        -- user/system/agent
    action        TEXT NOT NULL,                         -- 操作类型（如 tool_call / config_change）
    -- 工具调用专用字段（非工具操作时为 NULL）
    tool_name     TEXT,
    tool_source   TEXT,                                  -- builtin / mcp / skill
    risk_level    TEXT,
    params_digest TEXT,                                  -- 参数 SHA256 摘要（不存原始值）
    status        TEXT,                                  -- success / failed / rejected / timeout
    error_code    TEXT,
    duration_ms   INTEGER,
    result_bytes  INTEGER,
    truncated     INTEGER,                               -- 0/1
    approved_by   TEXT,                                  -- auto / human
    -- 通用字段
    target_type   TEXT,
    target_id     TEXT,
    detail_json   TEXT NOT NULL DEFAULT '{}',
    created_at    INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_audit_ws      ON audit_logs(workspace_id, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_tool    ON audit_logs(tool_name, created_at);
CREATE INDEX IF NOT EXISTS idx_audit_session ON audit_logs(session_id);

-- 审计日志不可篡改性（数据库级强制：禁止 UPDATE/DELETE）
CREATE TRIGGER IF NOT EXISTS audit_logs_no_update
BEFORE UPDATE ON audit_logs
BEGIN
    SELECT RAISE(ABORT, 'audit_logs 表为追加专用，禁止 UPDATE');
END;

CREATE TRIGGER IF NOT EXISTS audit_logs_no_delete
BEFORE DELETE ON audit_logs
BEGIN
    SELECT RAISE(ABORT, 'audit_logs 表为追加专用，禁止 DELETE。如需清理历史数据请使用 data_purge 命令');
END;

-- 慢工具告警视图（默认阈值 10s）
CREATE VIEW IF NOT EXISTS slow_tool_alerts AS
    SELECT * FROM audit_logs
    WHERE tool_name IS NOT NULL
      AND duration_ms > 10000;

-- 9b. 进化历史表
CREATE TABLE IF NOT EXISTS evolution_log (
    id          TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE,
    action_type TEXT NOT NULL,                           -- create_skill / refine_prompt / retire_skill
    payload     TEXT NOT NULL DEFAULT '{}',              -- JSON 快照
    status      TEXT NOT NULL DEFAULT 'applied'          -- applied / rolled_back / pending_approval
                CHECK(status IN ('applied','rolled_back','pending_approval')),
    approved_by TEXT,                                    -- NULL=自动，否则记录操作者
    created_at  INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_evolog_ws ON evolution_log(workspace_id, created_at);

-- 9. 任务执行轨迹（进化引擎数据源）
CREATE TABLE IF NOT EXISTS task_traces (
    id              TEXT PRIMARY KEY,
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    parent_id       TEXT REFERENCES task_traces(id) ON DELETE SET NULL,
    status          TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','done','failed','cancelled')),
    depth           INTEGER NOT NULL DEFAULT 0 CHECK(depth <= 3),
    input_json      TEXT,                    -- 任务输入参数（JSON）
    checkpoint_json TEXT,                    -- 覆盖写入最新 checkpoint（JSON）
    finished_at     INTEGER,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at      INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_traces_workspace ON task_traces(workspace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_traces_status ON task_traces(status) WHERE status = 'running';

-- 任务步骤详情（TraceStep 子表）
CREATE TABLE IF NOT EXISTS trace_steps (
    id           TEXT PRIMARY KEY,
    trace_id     TEXT NOT NULL REFERENCES task_traces(id) ON DELETE CASCADE,
    step_index   INTEGER NOT NULL,
    action_type  TEXT NOT NULL,               -- 'llm_call' | 'tool_call' | 'skill_call' | 'sub_agent' | 'plan'
    tool_name    TEXT,
    input_json   TEXT,                         -- 输入参数（截断到 10KB）
    output_json  TEXT,                         -- 输出结果（截断到 10KB）
    reasoning    TEXT,                         -- LLM CoT 推理过程
    duration_ms  INTEGER,
    token_count  INTEGER,
    is_error     INTEGER NOT NULL DEFAULT 0,
    error_msg    TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

CREATE INDEX IF NOT EXISTS idx_steps_trace ON trace_steps(trace_id, step_index);

-- 11. 多媒体任务表（TTS 除外，TTS 流式不落盘任务记录）
CREATE TABLE IF NOT EXISTS media_tasks (
    task_id      TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    modality     TEXT NOT NULL CHECK(modality IN ('asr','image','video','music','lyrics')),
    status       TEXT NOT NULL DEFAULT 'pending'
                 CHECK(status IN ('pending','processing','done','failed','cancelled')),
    progress_pct INTEGER CHECK(progress_pct BETWEEN 0 AND 100),
    file_paths   TEXT NOT NULL DEFAULT '[]',             -- JSON 数组，本地缓存绝对路径
    error        TEXT,
    provider     TEXT NOT NULL,                          -- "google" / "minimax"
    model        TEXT NOT NULL,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    finished_at  INTEGER
);
CREATE INDEX IF NOT EXISTS idx_media_tasks_ws     ON media_tasks(workspace_id, created_at);
CREATE INDEX IF NOT EXISTS idx_media_tasks_status ON media_tasks(status);

-- 12. AI 产物表（save_ai_artifact 命令写入）
CREATE TABLE IF NOT EXISTS ai_artifacts (
    id              TEXT PRIMARY KEY,
    conversation_id TEXT REFERENCES conversations(id) ON DELETE SET NULL,
    message_id      TEXT REFERENCES messages(id) ON DELETE SET NULL,     -- 关联源消息，支持跳回（F-10）
    workspace_id    TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    artifact_type   TEXT NOT NULL CHECK(artifact_type IN ('code','report','article','data','other')),
    title           TEXT NOT NULL,
    file_path       TEXT NOT NULL,                       -- 相对于 ai-docs/ 的路径
    mime_type       TEXT NOT NULL,
    size_bytes      INTEGER NOT NULL DEFAULT 0,
    created_at      INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_artifacts_ws   ON ai_artifacts(workspace_id, created_at);
CREATE INDEX IF NOT EXISTS idx_artifacts_conv ON ai_artifacts(conversation_id);

-- 13. 向量嵌入虚拟表（sqlite-vec vec0）
CREATE VIRTUAL TABLE IF NOT EXISTS embeddings USING vec0(
    embedding FLOAT[1536],           -- OpenAI/其他模型嵌入维度，按需调整
    +source_type TEXT,               -- 辅助列：来源类型（memory/message/skill/knowledge）
    +source_id   TEXT,               -- 辅助列：关联记录 UUID
    +workspace_id TEXT               -- 辅助列：所属工作区
);

-- 14. FTS5 全文检索虚拟表（跨会话历史搜索 F-17 / session_search 工具）
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,                         -- 消息正文（与 messages.content 同步）
    conversation_title,              -- 会话标题（冗余，避免 JOIN）
    content='messages',
    content_rowid='rowid',
    tokenize='unicode61'
);
-- 触发器保持 FTS 索引与 messages 表同步
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content, conversation_title)
    SELECT new.rowid, new.content,
           (SELECT title FROM conversations WHERE id = new.conversation_id);
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, conversation_title)
    VALUES ('delete', old.rowid, old.content, '');
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content, conversation_title)
    VALUES ('delete', old.rowid, old.content, '');
    INSERT INTO messages_fts(rowid, content, conversation_title)
    SELECT new.rowid, new.content,
           (SELECT title FROM conversations WHERE id = new.conversation_id);
END;

-- 15. 对话标签表（F-18 自定义颜色标签）
CREATE TABLE IF NOT EXISTS tags (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    color        TEXT NOT NULL DEFAULT '#6366f1',         -- CSS 颜色值
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    UNIQUE(workspace_id, name)
);

CREATE TABLE IF NOT EXISTS conversation_tags (
    conversation_id TEXT NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
    tag_id          TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (conversation_id, tag_id)
);

-- 16. Prompt 片段库（F-23）
CREATE TABLE IF NOT EXISTS prompt_snippets (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT REFERENCES workspaces(id) ON DELETE CASCADE, -- NULL=全局片段
    title        TEXT NOT NULL,
    content      TEXT NOT NULL,                          -- 支持 {{variable}} 占位符
    variables    TEXT NOT NULL DEFAULT '[]',             -- JSON 数组，变量声明
    tags         TEXT NOT NULL DEFAULT '[]',             -- JSON 数组，分类标签
    use_count    INTEGER NOT NULL DEFAULT 0,             -- 使用次数，排序用
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_snippets_ws ON prompt_snippets(workspace_id, use_count DESC);

-- 17. 定时任务表（F-24）
CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    skill_name   TEXT NOT NULL,                          -- 调用的 Skill 名称
    skill_input  TEXT NOT NULL DEFAULT '{}',             -- JSON，传给 Skill 的参数
    cron_expr    TEXT NOT NULL,                          -- cron 表达式（"0 9 * * 1-5"）
    is_enabled   INTEGER NOT NULL DEFAULT 1,
    last_run_at  INTEGER,
    next_run_at  INTEGER,                                -- 预计下次执行时间（Unix ms）
    last_status  TEXT CHECK(last_status IN ('success','failed','running',NULL)),
    last_error   TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);
CREATE INDEX IF NOT EXISTS idx_sched_next ON scheduled_tasks(workspace_id, next_run_at) WHERE is_enabled = 1;

-- ============================================================
-- 知识库（RAG）
-- ============================================================

-- 知识库知识源元数据
CREATE TABLE IF NOT EXISTS knowledge_sources (
    id           TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    source_type  TEXT NOT NULL DEFAULT 'markdown'  -- 'markdown' | 'pdf' | 'txt' | 'url' | 'code'
                 CHECK(source_type IN ('markdown','pdf','txt','url','code')),
    uri          TEXT,                              -- 原始文件路径或 URL
    content_hash TEXT,                              -- SHA-256，用于重复检测
    mime_type    TEXT,
    file_size    INTEGER,
    chunk_count  INTEGER NOT NULL DEFAULT 0,
    status       TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','processing','ready','failed')),
    error_message TEXT,
    created_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000),
    updated_at   INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

CREATE INDEX IF NOT EXISTS idx_knowledge_sources_workspace ON knowledge_sources(workspace_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_sources_hash ON knowledge_sources(workspace_id, content_hash) WHERE content_hash IS NOT NULL;

-- 知识库分块
CREATE TABLE IF NOT EXISTS knowledge_chunks (
    id            TEXT PRIMARY KEY,
    source_id     TEXT NOT NULL REFERENCES knowledge_sources(id) ON DELETE CASCADE,
    workspace_id  TEXT NOT NULL,
    chunk_index   INTEGER NOT NULL,               -- 块在文档中的顺序
    content       TEXT NOT NULL,
    token_count   INTEGER NOT NULL,               -- 真实 token 数（非字节数）
    embedding     BLOB,                           -- 向量嵌入（内联存储，不使用共享 embeddings 表）
    metadata_json TEXT DEFAULT '{}',              -- JSON: { page, section, parent_chunk_id }
    created_at    INTEGER NOT NULL DEFAULT (unixepoch('now','subsec')*1000)
);

CREATE INDEX IF NOT EXISTS idx_chunks_source ON knowledge_chunks(source_id, chunk_index);
CREATE INDEX IF NOT EXISTS idx_kchunks_workspace ON knowledge_chunks(workspace_id);

-- 知识库全文检索（jieba 中文分词）
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content,
    content='knowledge_chunks',
    content_rowid='rowid',
    tokenize='jieba'
);

-- FTS5 同步触发器
CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON knowledge_chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content) VALUES('delete', OLD.rowid, OLD.content);
    INSERT INTO chunks_fts(rowid, content) VALUES (NEW.rowid, NEW.content);
END;
```

> **RAG 向量嵌入**：知识库分块的向量存储内联于 `knowledge_chunks.embedding` BLOB 列，不使用共享的 `embeddings` vec0 虚拟表。KNN 检索直接查询 `knowledge_chunks.embedding` 列。共享 `embeddings` 表仅用于 memory / message / skill 等其他来源的向量索引。

---

## 三、各表设计说明

| 表名 | 用途 | 关键字段 |
| --- | --- | --- |
| workspaces | 隔离多项目配置，每个工作区独立管理资源 | root_path、config_json |
| conversations | 记录完整对话会话，汇总 token 消耗 | model、status、is_pinned、parent_conversation_id |
| messages | 存储逐条消息，支持 tool call 追踪和思考内容 | role、reasoning_content、is_pinned |
| memory_entries | Agent 长期记忆，支持过期与分类检索 | importance、expires_at、category |
| skills | Skill 注册表，workspace_id 为 NULL 时为全局 | trigger_patterns、entry_file、version(semver)、origin |
| prompt_versions | Prompt 版本历史，is_current 标记生效版本 | version、is_current、variables |
| cost_records | 精细化成本审计，支持按 provider/时间段汇总 | provider、cost_usd、input_tokens |
| budget_configs | 运行时可修改的预算配置（全局/工作区级） | scope、daily_limit_usd、warn_at_pct |
| audit_logs | 不可篡改操作日志，覆盖权限与配置变更 | actor、action、detail_json |
| task_traces | 任务执行轨迹（进化引擎数据源），支持子任务树 | input_json、checkpoint_json、updated_at |
| trace_steps | 任务步骤详情（TraceStep 子表） | trace_id、action_type、reasoning、duration_ms |
| evolution_log | 进化引擎操作历史，支持回滚与审批 | action_type、status、approved_by |
| media_tasks | 异步多媒体任务（图像/视频/音乐/ASR） | modality、status、file_paths |
| ai_artifacts | AI 产物归档（代码/报告/文章/数据） | artifact_type、file_path、message_id |
| tags | 对话自定义颜色标签定义（F-18） | name、color |
| conversation_tags | 对话 ↔ 标签多对多关联 | conversation_id、tag_id |
| prompt_snippets | Prompt 片段库，支持变量占位符（F-23） | content、variables、use_count |
| scheduled_tasks | 定时触发 Skill 任务（F-24） | cron_expr、skill_name、next_run_at |
| knowledge_sources | 知识库知识源元数据（RAG） | source_type、content_hash、status |
| knowledge_chunks | 知识库分块（RAG） | chunk_index、token_count、embedding、metadata_json |
| chunks_fts | FTS5 虚拟表，知识库全文检索（jieba 中文分词） | content |
| embeddings | vec0 虚拟表，支持 KNN 近似向量检索 | embedding(FLOAT[1536])、辅助列 |
| messages_fts | FTS5 虚拟表，跨会话全文检索（F-17） | content、conversation_title |
| daily_cost_summary | 成本按天 / Provider 聚合表（触发器自动维护） | workspace_id、provider、day、total_cost、call_count |

---

## 四、外键关系说明

```text
workspaces ──< conversations ──< messages ──< ai_artifacts（message_id, 可为 NULL）
           │                │              └─ messages_fts（FTS5 镜像，触发器维护）
           │                ├──< ai_artifacts（conversation_id，可为 NULL）
           │                └──< conversation_tags ──> tags（workspaces 管理）
           ├──< memory_entries（版本链：superseded_by 自引用）
           ├──< cost_records
           ├──< task_traces ──< task_traces（自引用，子任务）
           │                └──< trace_steps（步骤详情）
           ├──< skills ──< prompt_versions
           ├──< media_tasks
           ├──< tags ──< conversation_tags
           ├──< prompt_snippets
           ├──< scheduled_tasks
           ├──< knowledge_sources ──< knowledge_chunks ──< chunks_fts（FTS5 镜像，jieba 中文分词）
           └──< budget_configs（scope=workspace，可为 NULL=全局）
knowledge_chunks.embedding >── 内联 BLOB 向量（不使用共享 embeddings 表）
audit_logs >── workspaces（可为 NULL）
conversations >── conversations（自引用：parent_conversation_id 分支）
cost_records >── conversations（可为 NULL）
cost_records >── task_traces（可为 NULL）
```

删除策略：工作区删除时，会话、记忆、任务、标签、片段库随之级联删除（CASCADE）；cost_records 和 audit_logs 保留历史快照（SET NULL），不随关联记录消失。

---

## 四-b、详细设计阶段新增/变更的表

> :warning: 以下表在详细设计阶段各子系统中引入，尚未合并到上方权威 DDL。完整定义见各详细设计文档。

| 表名 | 来源文档 | 用途 |
| --- | --- | --- |
| `workflow_triggers` | [07-工作流触发器详细设计](../../04-详细设计阶段/_v0.3规划/07-工作流触发器详细设计.md) | 工作流触发器定义（cron/webhook/event） |
| `trigger_fire_history` | 同上 | 触发器触发历史记录 |
| `workflow_runs` | [05-工作流DAG执行引擎设计](../../04-详细设计阶段/_v0.3规划/05-工作流DAG执行引擎设计.md) | 工作流 DAG 执行记录 |
| `workflows` | [12-工作流编辑器详细设计](../../04-详细设计阶段/05-桌面端与交互/12-工作流编辑器详细设计.md) | 工作流定义（可视化编辑器） |
| `skill_versions` | [01-Skills系统详细设计](../../04-详细设计阶段/04-工具与扩展生态/01-Skills系统详细设计.md) | Skill 版本管理（细化原 skills 表） |
| `blob_refs` | [11-存储层详细设计](../../04-详细设计阶段/06-安全与基础设施/09-存储层详细设计.md) | 大对象引用（content-addressable） |
| `error_events` | [08-错误处理详细设计](../../04-详细设计阶段/06-安全与基础设施/06-错误处理详细设计.md) | 结构化错误事件持久化 |
| `knowledge_sources` | [04-知识库RAG详细设计](../../04-详细设计阶段/03-记忆与上下文/04-知识库RAG详细设计.md) | RAG 知识源（重构原 documents 表） |
| `knowledge_chunks` | 同上 | RAG 分块（重构原 document_chunks 表） |
| `spans` | [07-可观测性详细设计](../../04-详细设计阶段/06-安全与基础设施/05-可观测性详细设计.md) | OpenTelemetry 追踪 span 存储 |
| `usage_metrics` | 同上 | 使用指标聚合 |
| `model_catalog` | [11-设置与补充功能详细设计](../../04-详细设计阶段/05-桌面端与交互/11-设置与补充功能详细设计.md) | 模型目录缓存 |
| `daily_model_summary` | [05-模型洞察界面详细设计](../../04-详细设计阶段/02-Provider与模型层/05-模型洞察界面详细设计.md) | 模型使用日汇总 |
| `compare_sessions` | [02-多模型对比系统设计](../../04-详细设计阶段/02-Provider与模型层/02-多模型对比系统设计.md) | 多模型对比会话 |
| `eval_runs` / `eval_case_results` | [05-Agent评估系统设计](../../04-详细设计阶段/_v0.3规划/05-Agent评估系统设计.md) | Agent 评估套件运行与用例结果 |
| `marketplace_catalog` / `agent_ratings` | [06-Agent市场详细设计](../../04-详细设计阶段/_v0.3规划/06-Agent市场详细设计.md) | Agent 市场目录与评分 |
| `workspace_configs` / `workspace_mcp_servers` | [07-多工作区管理详细设计](../../04-详细设计阶段/05-桌面端与交互/07-多工作区管理详细设计.md) | 工作区配置与 MCP 服务器绑定 |
| `data_migrations` / `migration_backups` | [10-数据迁移详细设计](../../04-详细设计阶段/06-安全与基础设施/08-数据迁移详细设计.md) | 迁移记录与回滚备份 |
| `sync_devices` / `crypto_keys` | [03-E2E加密同步详细设计](../../04-详细设计阶段/_v0.3规划/03-E2E加密同步详细设计.md) | E2E 加密同步设备与密钥 |

> task_traces 表已按详细设计更新：`conversation_id` 改为 `NOT NULL / ON DELETE CASCADE`，移除进化引擎冗余字段（title / quality_score / user_rating 等），新增 `input_json`、`checkpoint_json`、`updated_at` 列。详见 [01-数据库访问层详细设计](../../04-详细设计阶段/06-安全与基础设施/01-数据库访问层详细设计.md)。

---

## 五、sqlite-vec vec0 语法说明

vec0 虚拟表声明向量列格式为 `列名 FLOAT[维度]`，辅助列（非向量）前加 `+` 前缀。KNN 查询示例：

```sql
SELECT source_id, source_type, distance
FROM embeddings
WHERE embedding MATCH ?          -- 绑定查询向量（BLOB/JSON 数组）
  AND workspace_id = ?
  AND k = 10;                    -- 返回最近邻 10 条
```

向量写入时将嵌入模型输出序列化为 JSON 数组或二进制 BLOB 绑定至 embedding 列，source_id 与 memory_entries / messages 的 id 对应（通过 source_type 区分：`memory` / `message` / `skill`），实现语义检索与精确记录的双向关联。知识库分块向量内联存储于 `knowledge_chunks.embedding` BLOB 列，不通过共享 `embeddings` 表。

> **时间戳字段规范**：项目标准为 `INTEGER NOT NULL`（Unix epoch 秒，通过 `unixepoch()` 生成）。历史表中部分使用 `unixepoch('now','subsec')*1000`（毫秒精度），新表统一采用秒精度。所有时间字段不使用 `TEXT` 类型。
