# Skills Update Tab v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 在 v1 更新 Tab 上落地「检查更新」：拉取远端 version/`updated_at`，标记 `outdated`，角标显示可更新数，默认筛选改为「可更新」；「全部更新」只处理 outdated。

**Architecture:** `skills` 新增 `check` 模块：由 origin 构造 `StoreSkill` → 复用 `fetch_detail` → 与 origin 记录的 `remote_*` / 本地时间戳比对；结果写回 origin 可选快照字段并返回；Tauri 暴露 `check_skill_updates`；前端 `skillUpdateRows` 区分 `outdated`/`current`，Updates Tab 切默认筛选与角标。

**Tech Stack:** Rust (`skills` + Tauri)、React (`SkillsPanel`)、`cargo test -p skills`、`node --experimental-strip-types --test`

**Spec:** [`docs/superpowers/specs/2026-07-15-skills-update-tab-design.md`](../specs/2026-07-15-skills-update-tab-design.md)（v2 行）

## Global Constraints

- **仅 v2**：不做本地改动备份/确认（v3）。
- **默认筛选**：`updatable`（= outdated）。
- **角标**：outdated 数量；0 时不显示数字。
- **全部更新**：只更新 `outdated`（无「检查」结果时不瞎全量强制装）。
- **无远端元数据**的源（如部分 skills.sh）：状态 `unknown`，不进默认「可更新」，仍可在「有来源」下手动更新。
- TDD；每 Task 单独 commit；在隔离 worktree/`feat/skills-update-v2` 上做。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `skills/src/check.rs` | 比对逻辑 + `check_one` / `check_all_for_agent` |
| Modify: `skills/src/models.rs` | `SkillUpdateCheckResult` DTO |
| Modify: `skills/src/lib.rs` | mod + re-export |
| Modify: `skills/src/install.rs` | 安装/更新成功后尽量写 `remote_version`/`remote_updated_at`（best-effort fetch） |
| Modify: `skills/src/update.rs` | `update_all_with_origin` 增加 `only_outdated: bool` 或新函数只更 outdated |
| Modify: `apps/desktop/src-tauri/src/skills_commands.rs` + `lib.rs` | `check_skill_updates`；update_all 传 only_outdated |
| Modify: `apps/desktop/src/types.ts` | CheckResult；UpdateRow status 扩展 |
| Modify: `apps/desktop/src/lib/skillUpdateRows.ts` (+ test) | outdated/current/unknown；filter `updatable` |
| Modify: `apps/desktop/src/components/SkillsPanel.tsx` | 检查更新按钮、角标、默认筛选、全部更新范围 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 检查中/完成文案 |
| Modify: spec 状态 → v2 已实现（最后 Task） |

---

### Task 1: 比对纯函数 + CheckResult DTO

**Files:**
- Modify: `skills/src/models.rs`
- Create: `skills/src/check.rs`
- Modify: `skills/src/lib.rs`

**Interfaces:**
```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillUpdateStatus {
    Outdated,
    Current,
    Unknown, // 无可用远端 version/updated_at
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpdateCheckResult {
    pub folder: String,
    pub status: SkillUpdateStatus,
    pub remote_version: Option<String>,
    pub remote_updated_at: Option<i64>,
    pub message: String,
}

/// 纯函数：用本地 origin 快照与远端快照判定状态。
pub fn classify_update_status(
    origin: &SkillOriginRecord,
    remote_version: Option<&str>,
    remote_updated_at: Option<i64>,
) -> SkillUpdateStatus
```

规则（实现与测试必须一致）：
1. 若 `remote_version` 与 `remote_updated_at` 皆空 → `Unknown`
2. 若两边都有 version 且字符串修剪后不等 → `Outdated`
3. 若两边都有 `updated_at` 且 `remote_updated_at > origin.remote_updated_at` → `Outdated`  
   （origin 无 `remote_updated_at` 时，用 `origin.last_updated_at.or(Some(origin.installed_at))` 作下限）
4. 否则若至少有一侧可比信息且未触发 outdated → `Current`
5. 仅一侧有 version、另一侧无：若仅有 remote version、本地无任何 remote_* 与时间 → 首次基线视为 `Current`（调用方随后应写回 origin，避免下次误 outdated）

- [x] **Step 1: 写失败测试**（`check.rs`）覆盖：version 不等；updated_at 更大；皆空 → Unknown；相等 → Current；无 remote_* 用 installed_at 作下限

- [x] **Step 2:** `cargo test -p skills check::classify` → RED

- [x] **Step 3:** 实现 `classify_update_status` + DTO + `pub mod check`

- [x] **Step 4:** GREEN → commit  
  `feat(skills): classify skill update status from remote metadata`

---

### Task 2: check_all — fetch_detail + 写回 origin

**Files:**
- Modify: `skills/src/check.rs`
- Test: 用 mock/`#[cfg(test)]` 注入 fetch，或测 `origin_to_store_skill` + classify 集成；网络路径可用 `wiremock` 若仓库已有，否则注入异步闭包。

**Interfaces:**
```rust
pub fn origin_to_store_skill(origin: &SkillOriginRecord) -> StoreSkill;

pub async fn check_updates_for_agent(
    agent_id: Option<&str>,
) -> Result<Vec<SkillUpdateCheckResult>>;
```

行为：
1. `load_origins`，过滤当前 agent；跳过本地无目录的孤儿（同 v1 update_all）。
2. 对每条 `fetch_detail(&origin_to_store_skill(o))`；失败 → status `Error`，message=err，不中断其它。
3. `classify_update_status`；随后 **upsert** origin：写入本次读到的 `remote_version`/`remote_updated_at`（供下次对比）。注意：若本次判定 `Outdated`，写回远端新值后本地仍视为需更新，直到用户执行 update；**classify 应在写回前用旧 origin 计算**，写回后保存新远端快照。前端以本次 API 返回的 status 为准（不要在写回后立刻再 classify 成 Current）。
4. 为避免「写回后变 Current」：origin 另存 `baseline_remote_version` / `baseline_remote_updated_at`（上次安装/更新时），或保留：
   - `remote_*` = 安装/更新时快照（baseline）
   - 检查结果不覆盖 `remote_*`，仅返回；  
   **本计划选定更简单方案：** 增加字段可选复杂；v2 用：
   - `remote_version` / `remote_updated_at` = **安装或成功更新时**的远端快照（baseline）
   - check **不写回** baseline；仅返回 CheckResult；前端持有 `lastCheckByFolder` map
   - install/`record_after_install` 在成功后 best-effort `fetch_detail` 填 baseline

- [x] **Step 1:** 实现 `origin_to_store_skill`（store/install_ref/name/id 从 origin 填）

- [x] **Step 2:** 实现 `check_updates_for_agent`（不覆盖 baseline remote_*）

- [x] **Step 3:** 单测：孤儿跳过；classify 路径用手动构造 origin+假远端（可抽 `check_origin_against_detail(origin, &StoreSkillDetail)`）

- [x] **Step 4:** commit  
  `feat(skills): check skill updates via store detail`

---

### Task 3: 安装成功写入 baseline remote_*

**Files:**
- Modify: `skills/src/install.rs` `record_after_install`

- [x] 成功安装后 `tokio` 内 best-effort：若能 `origin_to_store_skill` + `fetch_detail`，把 `version`/`updated_at` 写入 origin；失败忽略（baseline 空 → 之后 check 用 installed_at 规则或 Unknown）。

- [x] commit  
  `feat(skills): record remote baseline metadata after install`

---

### Task 4: update_all 仅 outdated

**Files:**
- Modify: `skills/src/update.rs`
- Modify: Tauri `update_all_skills(agent_id, only_outdated: Option<bool>)`

v2：`only_outdated == true`（前端默认）时：
1. 先 `check_updates_for_agent`（或接受前端传入 folder 列表）
2. 只对 `Outdated` 的 folder 调用 `update_installed_skill`

为少耦合：Tauri 新命令 `update_outdated_skills` 内部 check→filter→update；或 `update_all_skills` 增加参数。

- [x] 实现 + 单元测试：mock/构造 — 无 outdated 时返回空成功列表

- [x] commit  
  `feat(skills): update only outdated skills`

---

### Task 5: Tauri `check_skill_updates`

**Files:**
- `skills_commands.rs` / `lib.rs`

```rust
check_skill_updates(agent_id?) -> Vec<SkillUpdateCheckResult>
update_all_skills(agent_id?, only_outdated?: bool) // 默认 true for v2 callers
```

- [x] `cargo check -p astro-harness`
- [x] commit  
  `feat(tauri): expose check_skill_updates command`

---

### Task 6: 前端状态合并与筛选

**Files:**
- `types.ts`：`SkillUpdateStatus`、`SkillUpdateCheckResult`；`SkillUpdateRow.status`: `no_origin | with_origin | outdated | current | unknown`
- `skillUpdateRows.ts`：`applyCheckResults(rows, checks) -> rows`；`filterUpdateRows`：`updatable` ≡ `outdated`
- tests：outdated 进默认筛选；current 不进；unknown 不进 updatable

- [x] RED → GREEN → commit  
  `feat(frontend): apply remote check results to update rows`

---

### Task 7: SkillsPanel UI v2

**行为：**
- 默认 `updateFilter = "updatable"`
- Tab 角标：`outdatedCount`（>0 显示）
- 顶区按钮「检查更新」→ `check_skill_updates`，进行中 spinner；完成后 Toast「发现 N 个可更新」
- 「全部更新」只对 outdated（`only_outdated: true`）
- 有来源但 current：筛「有来源」可见，按钮文案可仍为「更新」（强制重装）
- 卡片标明 outdated 徽章（可选简洁圆点）

i18n：
```
skills.checkUpdates
skills.checkingUpdates
skills.checkUpdatesDone // "发现 {count} 个可更新"
skills.upToDate
skills.outdatedBadge
```

- [x] 实现 + `tsc -b`
- [x] commit  
  `feat(skills-ui): check updates badge and outdated-first filter`

---

### Task 8: 验收 + spec

- [x] `cargo test -p skills check::` / `update::` / `origins::`
- [x] node skillUpdateRows + skillInstalledMatch tests
- [x] spec 状态 → **v2 已实现**；链到本 plan
- [x] commit docs

---

## Spec coverage

| Spec v2 | Task |
|---------|------|
| check_skill_updates | T2–T5 |
| outdated vs current | T1, T6 |
| 默认筛选可更新 + 角标 | T7 |
| 全部更新仅 outdated | T4, T7 |
| 安装写 remote baseline | T3 |
| v3 本地改动 | 不做 |

## Out of scope

- 自动后台轮询检查
- 补录 install_ref
- 本地 diff / 备份确认（v3）
