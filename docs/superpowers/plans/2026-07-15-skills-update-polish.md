# Skills 更新体验打磨 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 打磨更新 Tab：进入自动检查、空态/已最新文案、`content_digest` 类型对齐，以及备份列表查看与打开。

**Architecture:** 前端进入 `updates` Tab 时自动 `check_skill_updates`（按 agent 做会话内去重 + 手动可再查）；卡片展示 current「已是最新」；后端列出 `~/.astro/skill-backups` 条目并暴露 reveal；Updates 页增加轻量「备份」区。

**Tech Stack:** 现有 SkillsPanel / skills crate / Tauri；TDD 偏向前端纯函数与 Rust 列目录。

**参考:** v1–v3 规格与终审 nits

## Global Constraints

- 自动检查：切换到 Updates Tab 或 agent 变更时触发；同一 agent 会话内成功检查后不重复刷（手动「检查更新」始终可强制）。
- 不做后台轮询、不做跨 Agent 批量。
- 备份管理 v1 范围：**列出 + 在文件管理器中打开**；删除可后续再加（YAGNI：若实现成本低可带简单删除）。
- 不改 classify 核心规则；不改 v3 确认流。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `SkillsPanel.tsx` | 自动检查、已最新徽章、备份区 UI |
| Modify: `skillUpdateRows.ts` (+test) | 可选：按 status 排序辅助 |
| Modify: `types.ts` | `content_digest`；`SkillBackupEntry` |
| Create: `skills/src/backups.rs` | `list_skill_backups` / `reveal_skill_backup` |
| Modify: Tauri commands + `lib.rs` | 暴露 list/reveal |
| Modify: i18n + `skills.css` | 文案与轻样式 |

---

### Task 1: 进入 Tab 自动检查

**Files:** `SkillsPanel.tsx`

- `useEffect`：当 `active && tab === "updates"`，若本 agent 尚无 `updateCheckResults`（或 agentId 变了），调用现有 `checkSkillUpdates`（抽公共函数，与按钮共用）。
- 用 `updateCheckMetaRef = { agentId, checkedAt }` 防重复。
- 手动按钮仍强制检查（忽略缓存）。
- 自动检查时可弱化 toast（仅手动检查弹「发现 N 个」），避免一进 Tab 就吵——**选定**：自动检查成功且 count>0 才 toast；count=0 不 toast；错误仍 toast。

- [ ] 实现 + 手动验思路写在 report
- [ ] commit `feat(skills-ui): auto-check updates when opening Updates tab`

---

### Task 2: 已最新徽章 + 空态文案

**Files:** `SkillsPanel.tsx`, `messages.ts`, `skills.css`

- `status === "current"` 卡片显示 `skills.upToDate` 徽章（与 outdated 对称）。
- `unknown` / `error` 在「有来源」筛选下可显示弱提示（optional 一行）：`skills.updateStatusUnknown` / `skills.updateCheckFailed`。
- 确认 `updatesNeedCheck` / `upToDate` 空态分支在自动检查后仍正确。

- [ ] commit `feat(skills-ui): show up-to-date badge on current skills`

---

### Task 3: TS `content_digest` 对齐

**Files:** `types.ts`

- `SkillOriginRecord` 增加可选 `content_digest?: string | null`

- [ ] commit `fix(types): add content_digest to SkillOriginRecord`

---

### Task 4: 后端列出 / 打开备份

```rust
pub struct SkillBackupEntry {
  pub agent_id: String,
  pub folder: String,
  pub timestamp: String, // 目录名
  pub path: String,
  pub created_at: Option<i64>, // 目录 mtime
}

pub fn list_skill_backups(agent_id: Option<&str>) -> Result<Vec<SkillBackupEntry>>;
pub fn reveal_skill_backup(path: &str) -> Result<()>; // 复用 open folder / reveal
```

排序：新→旧。过滤当前 agent（与 normalize 一致）。

- [ ] TDD：temp `ASTRO_MEMORY_DIR` 建 fake backups 结构后 list
- [ ] Tauri：`list_skill_backups` / `reveal_skill_backup`
- [ ] commit `feat(skills): list and reveal skill backups`

---

### Task 5: Updates 页备份区 UI

- Updates Tab 底部或折叠区「本地备份」：调用 `list_skill_backups`，刷新在检查/更新成功后。
- 每行：folder · 时间 · 「打开」按钮 → reveal
- 空：`skills.backupsEmpty`
- i18n zh/en

- [ ] commit `feat(skills-ui): show skill backup list on Updates tab`

---

### Task 6: 验收

- [ ] cargo test backups / 相关；tsc
- [ ] 短 docs 注记或更新 spec「体验打磨」小节（可选，prefer 在 design 加一小节）
- [ ] commit

---

## Out of scope

- 备份删除 / 清理策略
- 自动定时检查
- 补录 install_ref
