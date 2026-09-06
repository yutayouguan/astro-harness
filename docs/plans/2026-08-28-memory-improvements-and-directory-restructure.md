# Memory Improvements & Directory Restructure Plan

> 日期：2026-08-28 | **状态：已实施**
> 来源：Codex 对标分析后的三个借鉴点 + `~/.astro/` 目录整理
>
> 实施结果：
> - A1 Citation tracking → `crates/agent-memory/src/citation.rs`（companion 文件 `memory/usage.json`）
> - A2 Baselines → `crates/agent-memory/src/dreaming/mod.rs`（`workspace/memory/baselines/`）
> - A3 Polluted state → `AgentDreamStats.memory_hash + polluted` 字段
> - B 目录重组 → DB 归拢到 `data/`，记忆归拢到 `memory/`，workspace 保持不变

---

## Part A: 记忆系统借鉴改进

### A1: 记忆引用追踪（Citation）

**目标**：让模型在使用记忆时产生引用，追踪每条记忆的 `usage_count` 和 `last_used`，Dreaming 时用引用频率排序保留。

**当前状态**：MEMORY.md 和 USER.md 以 `§` 分隔条目注入 system prompt，模型可以读但无法标记「我用了哪条」。Dreaming 整页重写时 LLM 自行判断保留什么，无量化信号。

**设计**：

```rust
// crates/agent-memory/src/agent/store.rs — 扩展 MemoryEntry
pub struct MemoryEntry {
    pub text: String,
    pub id: String,              // 新增：稳定 ID（§ 分隔条目的 hash 或序号）
    pub usage_count: u32,        // 新增：引用计数
    pub last_used: Option<String>, // 新增：最后引用时间 (RFC3339)
    pub created_at: String,      // 新增：创建时间
}
```

**实施步骤**：

1. **条目 ID 方案**：每条 `§` 分隔条目生成稳定 ID（内容前 64 字符的 SHA256 前 8 位）。修改 `MemoryStore` 的解析/序列化逻辑，在 `§` 行后附加 `<!-- id:abc12345 usage:3 last:2026-08-28 -->` 元数据注释。
2. **引用注入**：system prompt 注入时，每条记忆前加 `[mem:abc12345]` 标签，模型可在回复中引用。
3. **引用解析**：Background Review 或 turn 结束后，扫描 assistant 消息中的 `[mem:xxx]` 模式，更新对应条目的 `usage_count` 和 `last_used`。
4. **Dreaming 排序**：整页重写前，将条目按 `usage_count DESC, last_used DESC` 排序后交给 LLM，prompt 中提示「频繁引用的记忆应优先保留」。

**影响范围**：`agent-memory`（store.rs, dreaming/）、`agent-core`（prompt/, memory_review.rs）

---

### A2: Git Baseline 变更追踪

**目标**：Dreaming 整页重写前建立 baseline，重写后生成 diff，方便审阅和回滚。

**当前状态**：Dreaming 通过 `MemoryStore::replace_all_entries()` 直接覆写 MEMORY.md，无变更记录。旧内容丢失。

**设计**：

在 `workspace/memory/` 下维护一个 **lightweight baseline 机制**（不引入 git 依赖，用文件快照）：

```
workspace/
├── MEMORY.md              # 当前 live 记忆
├── memory/
│   ├── baselines/         # 新增：变更基线
│   │   ├── 2026-08-28T01:00:00.md   # Dreaming 前快照
│   │   └── 2026-08-28T01:00:00.diff # 变更 diff
│   └── 2026-08-28.md      # 每日日记
```

**实施步骤**：

1. `finalize_dream_job()` 在写入前，将当前 MEMORY.md 复制到 `memory/baselines/{timestamp}.md`。
2. 写入后，生成简单的 unified diff 存为 `memory/baselines/{timestamp}.diff`。
3. 保留最近 5 个 baseline（轮转清理），支持 `restore_baseline(timestamp)` 回滚。
4. 如果 `write_approval=true`，pending 审批界面显示 diff 预览。

**影响范围**：`agent-memory`（dreaming/mod.rs, 新增 baselines 模块）

---

### A3: Polluted 脏状态标记

**目标**：当记忆文件被外部修改时标记脏状态，下次 Dreaming 时重新对齐。

**当前状态**：Dreaming 通过日期追踪已处理的日记（`dreaming.json` 中 `last_dreamed_date`），但如果用户手动编辑 MEMORY.md，Dreaming 不会感知。

**设计**：

```rust
// dreaming.json 扩展
pub struct DreamingState {
    pub last_dreamed_date: Option<String>,
    pub memory_hash: Option<String>,   // 新增：上次 Dreaming 后的 MEMORY.md hash
    pub polluted: bool,                // 新增：是否已被外部修改
    pub polluted_at: Option<String>,
}
```

**实施步骤**：

1. `finalize_dream_job()` 完成后，计算 MEMORY.md 的 SHA256 存入 `memory_hash`。
2. `select_undreamed_diaries()` 开始时，比对当前 MEMORY.md hash 与 `memory_hash`。不一致则标记 `polluted=true`。
3. Polluted 状态下，Dreaming prompt 附加提示：「MEMORY.md 在上次入梦后被修改过，请审慎保留用户手动编辑的内容」。
4. `memory` 工具写入（add/replace/remove）后，也更新 hash（不标记 polluted，因为这是受控修改）。

**影响范围**：`agent-memory`（dreaming/mod.rs）

---

## Part B: `~/.astro/` 目录整理

### 当前问题

1. **根目录杂乱**：10+ 文件 + 10+ 目录平铺在根级，无分类
2. **双重职责**：`agents/` 目录既存 Agent 运行时配置（`{id}/config.json`）又存角色定义（`*.toml`）
3. **记忆散落**：`audit/`、`learning/`、`pending/` 在根级，逻辑上属于记忆子系统
4. **数据库散落**：`usage.db`、`subagents-v2.db` 在根级，`state.db` 在 `sessions/`
5. **workspace 命名**：默认 Agent 工作区叫 `workspace/`，其他 `workspace-{id}/`——不一致

### 建议目录结构

```
~/.astro/
├── config.toml                    # 全局配置（不变）
├── active-agent.json              # 当前激活 Agent（不变）
│
├── agents/                        # Agent 角色定义（只放 .toml）
│   ├── default.toml               # 默认 Agent 定义
│   ├── worker.toml                # 内置 worker 角色
│   └── explorer.toml              # 内置 explorer 角色
│
├── workspaces/                    # Agent 工作区（统一命名）
│   ├── default/                   # 默认 Agent 工作区（原 workspace/）
│   │   ├── IDENTITY.md
│   │   ├── SOUL.md
│   │   ├── MEMORY.md
│   │   ├── USER.md
│   │   ├── memory/
│   │   │   ├── baselines/         # 新增：变更基线
│   │   │   └── 2026-08-28.md
│   │   ├── skills/
│   │   ├── assets/
│   │   └── generated/
│   └── {agent-id}/                # 其他 Agent 工作区
│       └── ...
│
├── data/                          # 数据库与持久化（归拢）
│   ├── state.db                   # SessionStore
│   ├── artifacts.db               # 文件空间索引
│   ├── knowledge.db               # Knowledge FTS
│   ├── usage.db                   # 用量事件
│   ├── subagents.db               # V2 Agent 线程图
│   └── cron.db                    # Cron 运行历史
│
├── memory/                        # 记忆子系统（归拢）
│   ├── dreaming.json              # 入梦全局状态
│   ├── pending/                   # 待审批写入
│   ├── audit/                     # 权限审计日志
│   └── learning/                  # DecisionLog
│
├── rollouts/                      # 会话历史 JSONL（不变）
│   └── YYYY/MM/DD/
│
├── skills/                        # 全局共享 Skills（不变）
├── cache/                         # 缓存（不变）
├── logs/                          # 日志（不变）
├── uploads/                       # 用户上传（不变）
├── cron/                          # Cron 任务定义（不变，只剩 jobs.json）
└── workflows/                     # 工作流定义（不变）
```

### 关键变更

| 变更 | 原路径 | 新路径 | 迁移方式 |
|---|---|---|---|
| 工作区统一 | `workspace/` | `workspaces/default/` | symlink 兼容 |
| 数据库归拢 | `sessions/state.db`, root `usage.db` 等 | `data/` | 启动时检测迁移 |
| 记忆归拢 | root `audit/`, `learning/`, `pending/`, `dreaming.json` | `memory/` | 启动时检测迁移 |
| Agent 定义分离 | `agents/{id}/config.json` + `agents/*.toml` | 定义在 `agents/`，运行时配置在 `workspaces/{id}/` | 启动时检测迁移 |
| 子 DB 文件名 | `subagents-v2.db` | `data/subagents.db` | 启动时检测迁移 |

### 迁移策略

1. **启动时检测**：检查旧路径是否存在，存在则迁移到新路径
2. **Symlink 兼容**：`workspace/` → `workspaces/default/` 的 symlink 保持旧路径可访问
3. **版本标记**：`~/.astro/version.json` 记录目录结构版本，避免重复迁移
4. **不删除旧文件**：迁移是 move + symlink，不是 copy + delete

---

## 实施顺序

| 阶段 | 内容 | 预计改动量 |
|---|---|---|
| **1** | A3 Polluted 脏状态标记 | 小（dreaming.json 扩展） |
| **2** | A1 记忆引用追踪 | 中（store 解析 + prompt 注入 + 引用扫描） |
| **3** | A2 Git Baseline | 中（baselines 目录 + diff 生成 + 轮转清理） |
| **4** | B 目录整理 | 大（path 常量 + 迁移逻辑 + 所有 crate 路径更新） |

先做 A1-A3（记忆改进），最后做 B（目录整理），因为目录整理会涉及大量路径重构。
