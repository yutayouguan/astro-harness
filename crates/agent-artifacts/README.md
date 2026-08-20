# artifacts

文件空间索引与知识内容检索 -- 管理 Agent 产出文件与用户上传文件的 SQLite 元数据索引，以及基于 FTS5 的文档正文检索。

属于 [Astro Agent](../../README.md) workspace，详见根目录 `CLAUDE.md` 的 Crate Map。

## 核心职责

1. **文件索引**（`artifacts.db`）-- 以绝对路径为唯一键，登记 Agent 写入、用户上传和磁盘扫描发现的文件元数据（名称、分类、大小、来源、Agent/会话关联），支持 UPSERT 与 missing 标记。
2. **磁盘对账** -- `reconcile` 扫描 `uploads/` 目录与各 Agent 工作区，自动补登记新文件、标记已删文件为 missing，跳过系统垃圾文件与 Agent 核心模板（SOUL.md/MEMORY.md 等）。
3. **知识内容检索**（`knowledge.db`）-- 文档标题 + 正文的 FTS5 全文检索，支持 `MATCH` 语法与 `LIKE` 回退，供 Agent 上下文召回和知识引用使用。
4. **分类过滤** -- 按扩展名自动归类为 `doc` / `image` / `code` / `sheet` / `av` / `pdf_ppt` / `other` 七类，支持按分类、关键词、时间、Agent 过滤查询。
5. **垃圾过滤** -- 自动排除 `.DS_Store`、`Thumbs.db`、`._*` 等系统/资源管理器垃圾文件，不入库且列表查询时排除。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | Crate 入口，re-export `ArtifactDb`、`KnowledgeDb` 及相关类型 |
| `db.rs` | `ArtifactDb` -- artifacts.db 的 SQLite 访问层：DDL 建表、register/get/list/reconcile/remove 等操作；路径归一化、扩展名分类、垃圾文件判定 |
| `content_db.rs` | `KnowledgeDb` -- knowledge.db 的 SQLite 访问层：FTS5 建表、register/search/delete、MATCH 失败回退 LIKE |

## 核心类型与 API

### ArtifactDb（文件索引）

```rust
pub struct ArtifactDb { conn: Connection, path: PathBuf }
```

主要方法：

| 方法 | 说明 |
|---|---|
| `new(path)` | 打开或创建数据库，旧库缺 `agent_id` 列时自动丢弃重建 |
| `register(path, source, session_id, message_id, agent_id)` | 登记/UPSERT 文件，返回 `ArtifactRow` |
| `get_by_path(path)` | 按路径查询单条记录 |
| `list(category, query, recent_only, limit, include_missing, agent_id)` | 多条件过滤列表查询 |
| `category_counts(include_missing, agent_id)` | 按分类统计数量 |
| `reconcile(memory_root)` | 磁盘对账：补登记 + 标记 missing，返回 `ReconcileReport` |
| `unlinked_paths()` | 列出未关联会话的文件路径（供回填） |
| `link_session_by_path(path, session_id, message_id)` | 为未关联文件回填会话 |
| `remove_by_paths(paths)` | 按路径批量删除索引行 |

### KnowledgeDb（知识检索）

```rust
pub struct KnowledgeDb { conn: Connection, path: PathBuf }
```

主要方法：

| 方法 | 说明 |
|---|---|
| `open(path)` / `open_default()` | 打开知识库（默认 `~/.astro/sessions/knowledge.db`） |
| `register(title, path, body, status)` | 登记文档并写入 FTS 正文；同 path 则更新 |
| `search(query, limit)` | FTS 检索，MATCH 失败时回退 LIKE 子串搜索 |
| `list(limit)` | 按时间倒序列出文档 |
| `get(id)` / `delete(id)` | 按 ID 查询/删除 |

### 关键结构体与枚举

- **`ArtifactRow`** -- 文件索引行：id/path/name/category/mime/size/source/session_id/message_id/agent_id/created_at/updated_at/missing
- **`ContentRow`** -- 知识文档行：id/title/path/status/created_at
- **`ArtifactSource`** -- 文件来源枚举：`AgentWrite` / `UserUpload` / `Reconcile`
- **`ReconcileReport`** -- 对账统计：`added`（新登记数）/ `marked_missing`（标记缺失数）

### 辅助函数

- `artifacts_db_path(memory_dir) -> PathBuf` -- 默认数据库路径
- `open_default(memory_dir) -> ArtifactDb` -- 打开默认数据库
- `category_from_name(name) -> &str` -- 按扩展名推断分类
- `is_junk_artifact_name(name) -> bool` -- 判断是否为系统垃圾文件

## 与其他 crate 的关系

- **`types`**（`agent-types`）-- 依赖 `SqliteStore` trait、`open_wal`、`delete_sqlite_files` 等 SQLite 工具函数
- **`home`**（`agent-home`）-- 使用 `default_memory_dir()` 获取 `~/.astro` 路径、`DEFAULT_AGENT_ID` 常量、`normalize_agent_id` / `agent_id_from_workspace_dir_name`
- **`agent-core`**（`agent`）-- Agent 运行时在工具执行后调用 `register` 登记写入文件，在 `run_turn` 中使用 `KnowledgeDb` 做上下文召回
- **`agent-tools`** -- 文件操作工具（write_file 等）完成后调用 `ArtifactDb::register` 入库
- **`agent-server`** -- gRPC 服务端暴露文件列表查询接口

## 测试运行命令

```bash
# 全部测试（单元 + 集成）
cargo test -p artifacts

# 仅单元测试（db.rs + content_db.rs 内嵌测试）
cargo test -p artifacts --lib

# 集成测试
cargo test -p artifacts --test artifact_test

# 带输出运行
cargo test -p artifacts -- --nocapture
```
