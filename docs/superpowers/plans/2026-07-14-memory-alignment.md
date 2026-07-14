# Memory Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Astro 记忆升级为有界精炼 `MemoryStore`、Frozen Snapshot、单一 `memory` 工具，并分两阶段交付审批/review/入梦 auxiliary。

**Architecture:** 用 `MemoryStore`（live + snapshot）替换 FIFO 的 `MemoryFile`；`AgentLoop` 只把 snapshot 注入 static prompt；工具面收敛为 `memory(action,target)`；日记仅系统/入梦写；P2 增加 pending 审批与 `auxiliary.*` 便宜模型。

**Tech Stack:** Rust workspace（`memory` / `agent` / `tools` / `hooks` config / Tauri）、serde_yaml、现有 SessionStore FTS、现有 dreaming 管线

**Spec:** [`docs/superpowers/specs/2026-07-14-memory-hermes-alignment-design.md`](../specs/2026-07-14-memory-hermes-alignment-design.md)

---

## File map

| Path | Responsibility |
|------|----------------|
| `memory/src/agent/store.rs`（新建） | `MemoryStore`：§ 解析、容量、扫描、去重、live/snapshot |
| `memory/src/agent/scan.rs`（新建） | 写入前威胁 / 不可见 Unicode 扫描 |
| `memory/src/agent/files.rs` | 删除或薄委托到 `store`（避免双实现） |
| `memory/src/agent/mod.rs` / `lib.rs` | re-export |
| `memory/src/session/manager.rs` | 持有 store、dispatch `memory`、refresh、日记 API |
| `memory/src/config.rs`（新建）或扩展 `hooks/src/config.rs` | `memory:` / `auxiliary:` 解析（单一 yaml） |
| `memory/src/dreaming/mod.rs` | finalize 经 MemoryStore；P2 aux 路由 |
| `memory/src/pending.rs`（P2） | write_approval 队列 |
| `memory/src/review.rs`（P2） | 回合后 review digest + 应用建议 |
| `agent/src/loop_.rs` / `context.rs` | Frozen Snapshot 注入；日记截断 |
| `tools/src/builtins/memory_tools.rs` | 注册单一 `memory` |
| `tools/src/core/dispatch.rs` | 工具名分支 |
| `memory/src/agent/tools_enabled.rs` | toolset 映射 |
| `agent/src/delegate_exec.rs` | deny/allow 名单工具名 |
| `frontend/src-tauri/src/*` | `refresh_memory`；P2 pending UI 命令 |
| `docs/hooks.md` 旁或 `docs` 记忆小节 | 用户可见配置说明（可选短节） |

**命名：** 代码与 UI **禁止**出现参考项目品牌字符串。

---

## Phase P1 — 内核

### Task 1: `MemoryStore` + scan（TDD）

**Files:**
- Create: `memory/src/agent/store.rs`
- Create: `memory/src/agent/scan.rs`
- Modify: `memory/src/agent/mod.rs`
- Modify: `memory/src/agent/files.rs`（改为 `pub use store::*` 或删除后改 import）
- Test: unit tests in `store.rs` / `scan.rs`

- [ ] **Step 1: Write failing tests for parse/serialize and overflow**

```rust
#[test]
fn parse_section_and_legacy_bullets() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("MEMORY.md");
    std::fs::write(&path, "- alpha\n- beta\n").unwrap();
    let store = MemoryStore::open(path.clone(), 2200).unwrap();
    assert_eq!(store.live_entries(), &["alpha", "beta"]);
    store.save_for_test(); // or add + save via public API
    let on_disk = std::fs::read_to_string(&path).unwrap();
    assert!(on_disk.contains('§'));
}

#[test]
fn add_fails_when_over_limit_without_dropping() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("MEMORY.md");
    let mut store = MemoryStore::open(path, 40).unwrap();
    store.add("short ok").unwrap();
    let before = store.live_entries().len();
    let err = store.add("this entry is intentionally far too long for the tiny limit").unwrap_err();
    assert!(err.to_string().contains("limit") || err.to_string().contains("上限"));
    assert_eq!(store.live_entries().len(), before);
}
```

- [ ] **Step 2: Run tests — expect FAIL (type missing)**

Run: `cargo test -p memory store:: -- --nocapture`  
Expected: compile error / no `MemoryStore`

- [ ] **Step 3: Implement minimal `MemoryStore`**

API 形状（钉死，后续 task 引用同一签名）：

```rust
pub struct MemoryStore {
    path: PathBuf,
    max_chars: usize,
    live: Vec<String>,
    snapshot: Vec<String>,
}

impl MemoryStore {
    pub fn open(path: PathBuf, max_chars: usize) -> anyhow::Result<Self>;
    pub fn reload(&mut self) -> anyhow::Result<()>; // disk → live + copy to snapshot
    pub fn refresh_snapshot(&mut self); // live → snapshot（不读盘）
    pub fn snapshot_render(&self) -> String; // usage header + § join
    pub fn live_render(&self) -> String;
    pub fn live_entries(&self) -> &[String];
    pub fn current_chars(&self) -> usize;
    pub fn add(&mut self, content: &str) -> anyhow::Result<MemoryWriteResult>;
    pub fn replace(&mut self, old_text: &str, content: &str) -> anyhow::Result<MemoryWriteResult>;
    pub fn remove(&mut self, old_text: &str) -> anyhow::Result<MemoryWriteResult>;
}

pub struct MemoryWriteResult {
    pub message: String,
    pub usage: String, // "1474/2200"
    pub duplicate: bool,
}
```

规则：
- 分隔符常量 `ENTRY_DELIMITER = "\n§\n"`
- `current_chars`：`entries.join(ENTRY_DELIMITER).chars().count()`（单测钉死）
- `add`：精确去重 → `Ok` + `duplicate: true`；超限 `bail` 带 entries
- `replace`/`remove`：子串命中数必须为 1
- save：atomic temp + rename；盘格式仅 `§`

- [ ] **Step 4: Implement `scan.rs`**

```rust
pub fn scan_memory_content(content: &str) -> Result<(), String>;
```

拦截不可见 Unicode + 至少：`ignore previous instructions`、`authorized_keys`、`curl`+`API`/`TOKEN` 类模式（case-insensitive）。`add`/`replace` 在写入前调用。

- [ ] **Step 5: Tests for unique substring, duplicate add, scan block**

- [x] **Step 6: `cargo test -p memory`**

Expected: PASS for new store/scan tests

- [x] **Step 7: Commit**

```bash
git add memory/src/agent/store.rs memory/src/agent/scan.rs memory/src/agent/mod.rs memory/src/agent/files.rs
git commit -m "$(cat <<'EOF'
feat(memory): add bounded MemoryStore with § entries and scan

EOF
)"
```

---

### Task 2: Config `memory.*` + wire `MemoryManager`

**Files:**
- Create: `memory/src/config.rs`（推荐：memory 自管 memory/auxiliary；hooks 继续只读 hooks，或两端共用同一 parse 函数）
- Modify: `hooks/src/config.rs` — 扩展 `AstroConfig` 字段 **或** 抽共享解析；禁止两套互相覆盖的 yaml 路径
- Modify: `memory/src/session/manager.rs`
- Modify: `memory/src/lib.rs`
- Test: `memory/src/config.rs` 或 manager 集成测

- [ ] **Step 1: Config types**

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
    #[serde(default = "default_true")]
    pub user_profile_enabled: bool,
    #[serde(default = "default_mem_limit")]
    pub memory_char_limit: usize, // 2200
    #[serde(default = "default_user_limit")]
    pub user_char_limit: usize, // 1375
    #[serde(default)]
    pub write_approval: bool, // P2 用；P1 解析保留
    #[serde(default = "default_daily_max")]
    pub daily_prompt_max_chars: usize, // 1024
}
```

从 `{base}/config.yaml` 读；缺省文件 → 全默认。

- [ ] **Step 2: `MemoryManager` 持有两个 `MemoryStore` + `MemoryConfig`**

```rust
pub struct MemoryManager {
    pub base_dir: PathBuf,
    pub agent_id: String,
    pub workspace_dir: PathBuf,
    pub memory: MemoryStore,
    pub user: MemoryStore,
    pub session_store: SessionStore,
    pub config: MemoryConfig,
}
```

`for_agent`：`load_memory_config(&base_dir)` → `MemoryStore::open(..., cfg.memory_char_limit)` / user limit。

- [ ] **Step 3: Replace `MemoryTarget`**

```rust
pub enum MemoryTarget { Memory, User }
```

删除 `Daily` / `Project`。`append_daily` 保留为非工具方法。

- [ ] **Step 4: `prompt_snapshot_with_daily`**

```rust
pub fn prompt_snapshot_with_daily(&self) -> (String, String, String) {
    let mem = if self.config.memory_enabled { self.memory.snapshot_render() } else { String::new() };
    let user = if self.config.user_profile_enabled { self.user.snapshot_render() } else { String::new() };
    let mut daily = self.daily_content_today();
    // truncate to daily_prompt_max_chars on char boundary
    (mem, user, daily)
}
```

保留 `prompt_content` 兼容调用方时改为 **snapshot** 语义，并在 rustdoc 标明。

- [ ] **Step 5: `refresh_memory_snapshot(&mut self)`** — `memory.reload()?; user.reload()?;` 或 live→snapshot 后若需跟盘则 reload

- [x] **Step 6: 更新所有 `MemoryTarget::Project` 编译点**

Run: `cargo check -p memory -p agent -p tools -p backend`  
Expected: 修到通过

- [x] **Step 7: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): wire MemoryManager to configurable MemoryStore

EOF
)"
```

---

### Task 3: 单一 `memory` 工具 + dispatch

**Files:**
- Modify: `tools/src/builtins/memory_tools.rs`
- Modify: `tools/src/core/dispatch.rs`
- Modify: `memory/src/session/manager.rs` (`dispatch_memory_tool`)
- Modify: `memory/src/agent/tools_enabled.rs`
- Modify: `agent/src/delegate_exec.rs`（工具名列表）
- Test: manager unit test or tools test

- [ ] **Step 1: Schema**

```rust
#[derive(Deserialize, JsonSchema)]
pub struct MemoryArgs {
    pub action: MemoryAction, // add | replace | remove
    pub target: MemoryTargetArgs, // memory | user  (serde rename_all lowercase)
    pub content: Option<String>,
    pub old_text: Option<String>,
}
```

只注册 **一个** `name: "memory"`，`toolset: "memory"`。保留 `session_search`。

- [ ] **Step 2: `dispatch_memory_tool`**

```rust
match name {
    "memory" => { /* action/target/content/old_text */ }
    "session_search" => { ... }
    "memory_add" | "memory_replace" | "memory_remove" => {
        anyhow::bail!("工具已迁移为 memory(action,target)；请使用 action=add|replace|remove")
    }
    _ => anyhow::bail!("未知记忆工具: {name}"),
}
```

`handle_memory_*` 合并为：

```rust
pub fn handle_memory_op(&mut self, action, target, content, old_text) -> Result<String>
```

成功消息注明「已写盘（live）；当前会话 prompt 快照未刷新」。

- [ ] **Step 3: `tool_name_to_toolset("memory") => "memory"`**；清理旧三名

- [x] **Step 4: `cargo test -p tools -p memory`**

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(tools): replace memory_* tools with single memory tool

EOF
)"
```

---

### Task 4: AgentLoop Frozen Snapshot

**Files:**
- Modify: `agent/src/loop_.rs`
- Modify: `agent/src/context.rs`（可选：render 标题带 usage，若 snapshot_render 已含 header 则避免双标题）
- Test: `agent/tests/agent_test.rs` 或新建 `agent/tests/memory_snapshot_test.rs`

- [ ] **Step 1: 构造时 snapshot 已由 `MemoryStore::open` 固化** — 确认 `open` = load disk → live + snapshot

- [ ] **Step 2: 改 `build_system_prompt`**

**禁止**每轮 `prompt_content_with_daily` 重读导致「写入立刻进 prompt」。应：

```rust
let (project_memory, user_profile, daily) = self.memory.prompt_snapshot_with_daily();
```

`daily` 截断已在 manager 完成；`StaticContext` 今日层标题改为标明流水，例如 `# 今日记忆（流水截断）`。

- [ ] **Step 3: `pub fn refresh_memory(&mut self) -> Result<()>`** → `self.memory.refresh_memory_snapshot()`

- [ ] **Step 4: 换 `session_id` 的路径**（`with_session_id` / 重置）调用 `refresh_memory`

- [ ] **Step 5: 集成测**

```rust
#[test]
fn snapshot_frozen_within_session() {
    // AgentLoop + temp dir
    // memory tool add entry
    // build_system_prompt 不含新条目
    // refresh_memory 后含新条目
}
```

- [x] **Step 6: `cargo test -p agent`**

- [x] **Step 7: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): freeze MEMORY/USER snapshot in system prompt

EOF
)"
```

---

### Task 5: Tauri `refresh_memory` + 入梦经 Store

**Files:**
- Modify: `frontend/src-tauri/src/lib.rs`（register command）
- Create or Modify: `frontend/src-tauri/src/memory_commands.rs`（若已有则扩展）
- Modify: `memory/src/dreaming/mod.rs` — `finalize_dream_job_with_memory`
- Modify: 前端记忆页（可选按钮「刷新进对话」）— 最小：命令可用即可

- [ ] **Step 1: Tauri command**

```rust
#[tauri::command]
fn refresh_memory(app_state: ...) -> Result<(), String> {
    // 对当前活跃 AgentLoop / MemoryManager 调 refresh
}
```

若当前架构是每请求新建 Manager：则 refresh = 下次构建时 reload（文档写清）；若长驻 `AgentLoop`，必须调实例方法。

- [ ] **Step 2: 入梦 finalize 禁止直接 `write_text_atomic(MEMORY.md)` 整文件覆盖跳过门禁**

改为：用 `MemoryStore::open` 加载 → 将模型输出解析为条目列表（或整页 replace 策略见下）→ 经 scan/limit。

**入梦写回策略（钉死）：**  
模型仍产出「完整 MEMORY Markdown」。实现：

1. 解析输出为条目（`§` 或 `- `）  
2. `MemoryStore::open` 现有文件  
3. 用新条目列表 **整体替换 live** 的专用方法 `replace_all_entries(Vec<String>) -> Result<()>`：先算总字符，超限失败；逐条 scan；成功则 save；**不**更新调用方 AgentLoop 的 snapshot  

```rust
pub fn replace_all_entries(&mut self, entries: Vec<String>) -> anyhow::Result<()>;
```

- [ ] **Step 3: 更新 `count_memory_bullets` 等统计适配 `§`**

- [x] **Step 4: `cargo test -p memory` dreaming 相关 + tauri check**

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): route dreaming writes through MemoryStore; add refresh_memory

EOF
)"
```

---

### Task 6: P1 文档与回归清单

**Files:**
- Modify: `docs/superpowers/specs/2026-07-14-memory-hermes-alignment-design.md` 状态 → 部分实现  
- Create or Modify: 简短用户说明（`docs/memory.md` 或 README 一节）— 配置键、`memory` 工具、`refresh`

- [x] **Step 1: 手测清单写入计划备注并执行**
  - 旧 `- ` MEMORY 可加载并在首次写入后变 `§`
  - 超限工具错误含 usage
  - `session_search` 仍可用
  - 无品牌禁用名泄漏（`rg -i hermes` 仅 spec 外链）

- [ ] **Step 2: `cargo test -p memory -p agent -p tools`**

- [ ] **Step 3: Commit docs**

---

## Phase P2 — 学习闭环

### Task 7: `write_approval` pending 队列

**Files:**
- Create: `memory/src/pending.rs`
- Modify: `memory/src/session/manager.rs` — 写路径分支
- Modify: Tauri commands + 设置/记忆页最小 UI
- Test: `memory/tests/pending_test.rs`

- [ ] **Step 1: Pending 记录**

```rust
pub struct PendingMemoryWrite {
    pub id: String,
    pub agent_id: String,
    pub target: MemoryTarget, // Memory | User
    pub action: String,
    pub content: Option<String>,
    pub old_text: Option<String>,
    pub source: String, // "tool" | "review" | "dreaming"
    pub created_at: String,
}
```

路径：`{base}/pending/memory/{id}.json`

- [ ] **Step 2: `config.write_approval == true` 时** `handle_memory_op` / dreaming / review → `enqueue` 不改 live  
- [ ] **Step 3: `approve(id)` / `reject(id)` / `list_pending()`  
- [ ] **Step 4: UI 最小列表  
- [ ] **Step 5: Tests + commit**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): add write_approval pending queue

EOF
)"
```

---

### Task 8: `auxiliary` + background review + dreaming 模型路由

**Files:**
- Modify: `memory/src/config.rs` — `AuxiliaryConfig`
- Create: `memory/src/review.rs` 或 `agent/src/memory_review.rs`
- Modify: `agent` turn 结束路径（streaming / loop 成功后 `tokio::spawn`）
- Modify: `frontend/src-tauri/src/dreaming_commands.rs` — 用 aux 模型调 LLM
- Test: review 用 mock/scripted provider

- [ ] **Step 1: Config**

```yaml
auxiliary:
  background_review:
    provider: auto
    model: auto
  dreaming:
    provider: auto
    model: auto
```

- [ ] **Step 2: `resolve_auxiliary(route, session_provider, session_model) -> (provider, model)`**

- [ ] **Step 3: Review**
  - 输入：最近 N=6 轮原文 + 更早摘要（截断）
  - 输出：JSON 数组建议 `{action,target,content?,old_text?}` + optional `daily_note`
  - 应用：`handle_memory_op` / `append_daily`（走审批）

- [ ] **Step 4: Dreaming command 改用 `auxiliary.dreaming`**

- [ ] **Step 5: 通知** — 可选事件 `memory_updated`（已有 event bus 则可复用）；默认一行，可后续做 display 开关

- [ ] **Step 6: Tests + commit**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): auxiliary review and dreaming model routes

EOF
)"
```

---

## Spec coverage check

| Spec 要求 | Task |
|-----------|------|
| MemoryStore § / 超限报错 / 去重 / 扫描 | 1 |
| 可配 2200/1375 | 2 |
| Frozen Snapshot / refresh / 新 session | 4, 5 |
| 单一 memory 工具、无 read、子串 | 3 |
| 日记不进工具、截断注入 | 2, 4 |
| session_search 第二轨 | 3（保留） |
| 入梦经 Store | 5 |
| write_approval | 7 |
| background review + aux dreaming | 8 |
| 无外部 provider / 不改 SessionStore schema | （不做） |
| 禁止品牌名 | 全任务 + Task 6 rg |

---

## 执行说明

- **先做完 Task 1–6（P1）再开 P2**；每 task 提交一次。  
- TDD：红 → 绿 → 重构；不要跳过「先失败再实现」。  
- 若 `hooks::AstroConfig` 与 `memory::MemoryConfig` 合并困难：允许 `memory` 独立 `serde_yaml::from_str` 只反序列化自己需要的字段（`#[serde(default)]` 忽略未知键），hooks 同理 — **同一文件**、两处读。  
