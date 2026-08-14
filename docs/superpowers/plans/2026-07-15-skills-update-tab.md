# Skills Update Tab (v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 Skills 面板落地「更新」Tab（v1）：安装时写入 origin 清单，支持按原 `install_ref` 单条/批量强制重装；无来源技能可见但不可更新。

**Architecture:** 在 `skills` crate 新增 `origins` 模块（`~/.astro/skill-origins.json`，可按 agent 过滤）；`install_from_ref` / 商店安装成功后 upsert；新增 `update_installed_skill` / `update_all_with_origin` 复用现有安装路径；Tauri 暴露 list/update；前端合并扫盘列表与 origin，增加第 4 个主 Tab「更新」。

**Tech Stack:** Rust (`skills` + Tauri commands)、React (`SkillsPanel`)、`node --experimental-strip-types --test`、`cargo test -p skills`

**Spec:** [`docs/superpowers/specs/2026-07-15-skills-update-tab-design.md`](../specs/2026-07-15-skills-update-tab-design.md)

## Global Constraints

- **本期仅 v1**：强制重装；不做远端 version 对比、不做本地改动备份确认（v2/v3）。
- **默认筛选**：「有来源」；**角标**：v1 不显示数字计数。
- **覆盖语义**：更新 = 再跑 `install_from_ref`；UI 文案须标明将覆盖本地文件。
- **匹配**：folder / slug / name，与 `skillInstalledMatch` 一致。
- 跟随当前 Agent 选择器；不跨 Agent 一次全更。
- 每个 Task 结束单独 commit；TDD：先红后绿。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `skills/src/origins.rs` | 读写 origin、upsert、按 agent/folder 查找、推断 store/folder |
| Create: `skills/src/update.rs` | `update_one` / `update_all` 串行重装 |
| Modify: `skills/src/models.rs` | `SkillOriginRecord`、`SkillUpdateResult` DTO |
| Modify: `skills/src/install.rs` | 安装成功后写 origin；可选传入 name/store/folder hint |
| Modify: `skills/src/lib.rs` | mod + re-export |
| Modify: `frontend/src-tauri/src/skills_commands.rs` | `list_skill_origins` / `update_installed_skill` / `update_all_skills`；扩展 `install_store_skill` 参数 |
| Modify: `frontend/src-tauri/src/lib.rs` | 注册新 commands |
| Modify: `apps/desktop/src/types.ts` | Origin / UpdateResult 类型 |
| Create: `apps/desktop/src/lib/skillUpdateRows.ts` | 扫盘 + origin → 更新行 + 筛选 |
| Create: `apps/desktop/src/lib/skillUpdateRows.test.mjs` | 匹配与筛选单测 |
| Modify: `apps/desktop/src/components/SkillsPanel.tsx` | 更新 Tab、全部更新、已安装/本机次要更新按钮 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 中英文案 |
| Modify: `apps/desktop/src/styles/skills.css` | 筛选芯片 / 更新按钮态（必要时） |

---

### Task 1: Origin 模型与持久化模块

**Files:**
- Create: `skills/src/origins.rs`
- Modify: `skills/src/models.rs`
- Modify: `skills/src/lib.rs`
- Test: `skills/src/origins.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `SkillOriginRecord { folder, skill_id, name, store, install_ref, agent_id, scope, installed_at, last_updated_at, remote_version, remote_updated_at }`
  - `SkillOriginsFile { version: 1, records: Vec<SkillOriginRecord> }`
  - `fn origins_path() -> PathBuf` → `memory_dir()/skill-origins.json`
  - `fn load_origins() -> Result<SkillOriginsFile>`
  - `fn save_origins(file: &SkillOriginsFile) -> Result<()>`
  - `fn upsert_origin(record: SkillOriginRecord) -> Result<()>`（同 `agent_id`+`folder` 合并；更新时刷新 `last_updated_at`）
  - `fn find_origin(agent_id: Option<&str>, folder: &str) -> Result<Option<SkillOriginRecord>>`
  - `fn infer_store(install_ref: &str) -> String`
  - `fn infer_folder(install_ref: &str) -> Option<String>`

- [ ] **Step 1: 在 `models.rs` 增加 DTO**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillOriginRecord {
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_id: Option<String>,
    pub name: String,
    pub store: String,
    pub install_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub installed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_updated_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillOriginsFile {
    pub version: u32,
    pub records: Vec<SkillOriginRecord>,
}
```

- [ ] **Step 2: 写失败测试（临时目录）**

在 `origins.rs` 测试中设 `ASTRO_MEMORY_DIR` 临时目录：

```rust
#[test]
fn upsert_same_folder_updates_not_duplicates() {
    // tempdir + set ASTRO_MEMORY_DIR
    upsert_origin(SkillOriginRecord { folder: "ppt-generator-skill".into(), name: "a".into(), store: "skillhub".into(), install_ref: "skillhub:x/ppt-generator-skill".into(), agent_id: Some("workspace".into()), installed_at: 1, ..Default::default() }).unwrap();
    upsert_origin(SkillOriginRecord { folder: "ppt-generator-skill".into(), name: "a".into(), store: "skillhub".into(), install_ref: "skillhub:x/ppt-generator-skill".into(), agent_id: Some("workspace".into()), installed_at: 1, last_updated_at: Some(2), .. }).unwrap();
    let file = load_origins().unwrap();
    assert_eq!(file.records.len(), 1);
    assert_eq!(file.records[0].last_updated_at, Some(2));
}

#[test]
fn infer_folder_from_skillhub_and_clawhub() {
    assert_eq!(infer_folder("skillhub:owner/ppt-generator-skill").as_deref(), Some("ppt-generator-skill"));
    assert_eq!(infer_folder("clawhub:steipete--weather").as_deref(), Some("weather"));
}
```

（若 `SkillOriginRecord` 无 `Default`，测试里手填其余字段为 `None`。）

- [ ] **Step 3: Run 确认失败**

Run: `cargo test -p skills upsert_same_folder -- --nocapture`  
Expected: FAIL（module/fn 不存在）

- [ ] **Step 4: 实现 `origins.rs` 并挂到 `lib.rs`**

- 持久化路径对齐 `installed.rs` 的 `memory_dir()`（复制同函数或抽到共享小模块；为少扰动，可在 `origins.rs` 复制 `memory_dir`，与现有一致）。
- `upsert` 键：`(agent_id.normalize, folder)`；`agent_id` 缺省与 `"workspace"` 视为同一键时与 install 的 normalize 一致。
- 空文件/不存在 → `{ version: 1, records: [] }`。

- [ ] **Step 5: Run 确认通过**

Run: `cargo test -p skills origins:: -- --nocapture`  
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add skills/src/origins.rs skills/src/models.rs skills/src/lib.rs
git commit -m "feat(skills): add skill-origins.json persistence"
```

---

### Task 2: 安装成功后写入 origin

**Files:**
- Modify: `skills/src/install.rs`
- Modify: `frontend/src-tauri/src/skills_commands.rs`（可选参数透传）
- Test: `skills/src/install.rs` 或 `origins` + 轻量纯函数测 `record_after_install`

**Interfaces:**
- Consumes: `upsert_origin`, `infer_folder`, `infer_store`
- Produces:
  - `pub struct InstallOriginHint { pub name: Option<String>, pub store: Option<String>, pub folder: Option<String> }`
  - `install_from_ref(install_ref, agent_id, hint: Option<InstallOriginHint>) -> Result<String>`  
    （保持兼容：现有调用处传 `None`；或新增重载 `install_from_ref_with_origin`）
  - 成功后：`folder = hint.folder.or_else(|| infer_folder(ref))`；若仍无 folder，打 `tracing::warn!` 且不写清单（不 fail 安装）

- [ ] **Step 1: 写测试 `record_after_install_writes_origin`**

用临时 `ASTRO_MEMORY_DIR`，直接测公开 helper：

```rust
#[test]
fn record_after_install_upserts() {
    // set env temp
    record_after_install(
        "skillhub:owner/demo-skill",
        Some("workspace"),
        &InstallOriginHint {
            name: Some("Demo".into()),
            store: Some("skillhub".into()),
            folder: Some("demo-skill".into()),
        },
    ).unwrap();
    let o = find_origin(Some("workspace"), "demo-skill").unwrap().unwrap();
    assert_eq!(o.install_ref, "skillhub:owner/demo-skill");
    assert_eq!(o.store, "skillhub");
    assert_eq!(o.name, "Demo");
}
```

- [ ] **Step 2: Run 确认失败** → implement `record_after_install` → PASS

- [ ] **Step 3: 在 `install_from_ref` 成功路径末尾调用 `record_after_install`**

SkillHub HTTP 分支与 npx 分支都调用。`hint` 参数加入签名后，更新所有 crate 内调用点（`seed` 可传 `None`）。

- [ ] **Step 4: 扩展 Tauri `install_store_skill`**

```rust
pub async fn install_store_skill(
    install_ref: String,
    agent_id: Option<String>,
    name: Option<String>,
    store: Option<String>,
    folder: Option<String>,
) -> Result<String, String>
```

前端 `installSkill` 传入 `skill.name` / `skill.store` / 由 `install_ref`/`id` 解析的 folder（可用 TS `storeSkillMatchKeys` 的最后一个 slug，或新增 `inferFolderFromInstallRef` 与 Rust 对齐）。

- [ ] **Step 5: Commit**

```bash
git commit -m "feat(skills): persist origin on successful install"
```

---

### Task 3: update_one / update_all API

**Files:**
- Create: `skills/src/update.rs`
- Modify: `skills/src/models.rs`（`SkillUpdateItemResult`）
- Modify: `skills/src/lib.rs`
- Test: `skills/src/update.rs` 单元测试（mock：用可注入闭包或测「无 origin → Err」+「有 origin 调到 install 前」分层）

**Interfaces:**
- Produces:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdateItemResult {
    pub folder: String,
    pub ok: bool,
    pub message: String,
}

pub async fn update_installed_skill(
    agent_id: Option<&str>,
    folder: &str,
) -> Result<String>; // 无 origin → Err("无法追溯安装源: {folder}")

pub async fn update_all_with_origin(
    agent_id: Option<&str>,
) -> Result<Vec<SkillUpdateItemResult>>; // 串行；单条失败记入 Vec，不中断
```

- [ ] **Step 1: 失败测试 — 无 origin**

```rust
#[tokio::test]
async fn update_without_origin_errors() {
    // temp ASTRO_MEMORY_DIR, empty origins
    let err = update_installed_skill(Some("workspace"), "missing").await.unwrap_err();
    assert!(err.to_string().contains("无法追溯"));
}
```

- [ ] **Step 2: 实现：`find_origin` → `install_from_ref(ref, agent, hint from record)` → 更新 `last_updated_at`**

- [ ] **Step 3: `update_all_with_origin`：过滤当前 `agent_id` 的 records，逐条 `update_installed_skill`，收集结果**

（集成安装成功路径用已有 SkillHub/npx 过重；此 Task 以无 origin + origin upsert 后「至少调到 install」可用 `#[cfg(test)]` 注入，或接受 update 在真实 env 才绿。最低要求：无 origin Err + all 空列表返回 `Ok([])`。）

```rust
#[tokio::test]
async fn update_all_empty_ok() {
    let r = update_all_with_origin(Some("workspace")).await.unwrap();
    assert!(r.is_empty());
}
```

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(skills): update installed skill from origin"
```

---

### Task 4: Tauri 命令注册

**Files:**
- Modify: `frontend/src-tauri/src/skills_commands.rs`
- Modify: `frontend/src-tauri/src/lib.rs`

**Interfaces:**
- `list_skill_origins(agent_id?) -> Vec<SkillOriginRecord>`
- `update_installed_skill(folder, agent_id?) -> String`
- `update_all_skills(agent_id?) -> Vec<SkillUpdateItemResult>`

- [ ] **Step 1: 实现三命令，错误 `.map_err(|e| e.to_string())`**
- [ ] **Step 2: 在 `lib.rs` `invoke_handler` 注册**
- [ ] **Step 3: `cargo check -p astro-app` 或当前 Tauri 包名通过**
- [ ] **Step 4: Commit**

```bash
git commit -m "feat(tauri): expose skill origin list and update commands"
```

---

### Task 5: 前端合并行与筛选纯函数

**Files:**
- Create: `apps/desktop/src/lib/skillUpdateRows.ts`
- Create: `apps/desktop/src/lib/skillUpdateRows.test.mjs`
- Modify: `apps/desktop/src/types.ts`

**Interfaces:**
```ts
export type SkillUpdateFilter = "with_origin" | "no_origin" | "updatable";
// v1: updatable === with_origin（预留 v2 outdated）

export type SkillUpdateRow = {
  skill: InstalledSkill;
  origin: SkillOriginRecord | null;
  status: "with_origin" | "no_origin";
};

export function mergeUpdateRows(
  installed: InstalledSkill[],
  linkedMachine: InstalledSkill[],
  origins: SkillOriginRecord[],
  agentId: string,
): SkillUpdateRow[];

export function filterUpdateRows(
  rows: SkillUpdateRow[],
  filter: SkillUpdateFilter,
): SkillUpdateRow[];
```

匹配：origin.folder ↔ `skill.id` 末段；或 `origin.name` ↔ `skill.name`（小写）。仅纳入当前 agent 的 origin（`!origin.agent_id || origin.agent_id === agentId || (workspace 规范化)`）。

- [ ] **Step 1: 写测试（真实 ppt 案例）**

```js
test("folder matches origin when frontmatter name differs", () => {
  const rows = mergeUpdateRows(
    [{ id: "/tmp/skills/ppt-generator-skill", name: "ppt-generator", ... }],
    [],
    [{ folder: "ppt-generator-skill", name: "ppt-generator-skill", store: "skillhub", install_ref: "skillhub:x/ppt-generator-skill", agent_id: "workspace", installed_at: 1 }],
    "workspace",
  );
  assert.equal(rows[0].status, "with_origin");
});

test("default filter with_origin hides no_origin", () => {
  // ...
});
```

- [ ] **Step 2: RED → 实现 → GREEN**

Run: `node --experimental-strip-types --test apps/desktop/src/lib/skillUpdateRows.test.mjs`

- [ ] **Step 3: Commit**

```bash
git commit -m "feat(frontend): merge installed skills with origins for update tab"
```

---

### Task 6: SkillsPanel「更新」Tab UI（v1）

**Files:**
- Modify: `apps/desktop/src/components/SkillsPanel.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/styles/skills.css`（筛选芯片，若无现成 class）

**行为：**
- `SkillsTab` 增加 `"updates"`；主 Tab 顺序：已安装 | 本机 | **更新** | 在线。
- v1 不渲染数字角标（可仅 icon）。
- 进 Tab / 面板 active / agent 变更：并行 `list_skill_origins` + 已有 installed/machine 刷新。
- 默认 `updateFilter = "with_origin"`。
- 顶区文案：说明「更新将覆盖本地文件」+ 「全部更新」按钮（仅对有来源行）。
- 卡片：有来源 → 主按钮「更新」；进行中 `LoaderCircle` + `is-spin`；无来源 → disabled + title。
- 单条成功 Toast（可用 success tone）；失败 Toast error；全部更新后汇总「成功 N / 失败 M」。

文案键（中/英成对）：

```
skills.tab.updates
skills.updatesTitle
skills.updatesSub
skills.updatesFilter.updatable
skills.updatesFilter.withOrigin
skills.updatesFilter.noOrigin
skills.update
skills.updating
skills.updateAll
skills.updateDone
skills.updateAllDone  // "{ok} 成功，{fail} 失败"
skills.noOriginHint
skills.updateOverwriteHint
```

- [ ] **Step 1: i18n 键**
- [ ] **Step 2: Tab + 列表 + 调用 update commands**
- [ ] **Step 3: 手动验：装一个新 skill → 出现在「有来源」；点更新再装一次；旧无 origin 在「无来源」**
- [ ] **Step 4: Commit**

```bash
git commit -m "feat(skills-ui): add Updates tab with reinstall actions"
```

---

### Task 7: 已安装 / 本机次要「更新」入口

**Files:**
- Modify: `apps/desktop/src/components/SkillsPanel.tsx`

- [ ] **Step 1: `renderInstalledCard` / `renderMachineCard`：若该 skill 能 match 到 origin，显示次要按钮「更新」，调用同一 `update_installed_skill`**
- [ ] **Step 2: 无 origin 不显示按钮（避免噪音）**
- [ ] **Step 3: Commit**

```bash
git commit -m "feat(skills-ui): add update action on installed and machine cards"
```

---

### Task 8: 验收与规格状态回写

- [ ] **Step 1: 跑测试**

```bash
cargo test -p skills origins:: update::
node --experimental-strip-types --test apps/desktop/src/lib/skillUpdateRows.test.mjs apps/desktop/src/lib/skillInstalledMatch.test.mjs
```

Expected: all PASS

- [ ] **Step 2: 将 spec 状态改为「v1 已实现」**（若代码已合入），或保持「待实现」并在文末加「实现计划」链接  
  文件：`docs/superpowers/specs/2026-07-15-skills-update-tab-design.md`

- [ ] **Step 3: Commit docs if changed**

```bash
git commit -m "docs(skills): link update-tab plan and mark v1 status"
```

---

## Spec coverage (self-review)

| Spec 项 | Task |
|---------|------|
| origin 清单结构与路径 | T1 |
| 安装后写入 | T2 |
| update / update_all + 无来源错误 | T3–T4 |
| 更新 Tab、筛选默认有来源、无 v1 角标数字 | T6 |
| 覆盖文案 | T6 i18n |
| 已安装/本机次要更新 | T7 |
| folder/name 匹配 | T5（+ 既有 skillInstalledMatch） |
| v2/v3 | **明确不在本计划** |

## Out of scope (do not implement in this plan)

- `check_skill_updates` / outdated 角标
- 本地改动备份与确认对话框
- 手动补录 install_ref
- 跨 Agent 批量更新
