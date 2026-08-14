# Skills Update Tab v3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 更新前检测本地改动并明确提示；有改动时先备份再覆盖；更新失败支持一次自动重试并汇总。

**Architecture:** 安装/更新成功时为技能目录写入 `content_digest`（相对路径 + 文件 sha256 的稳定摘要）到 origin；更新前重算 digest，与 baseline 比较得到 `has_local_changes`；后端 `update_installed_skill` 支持 `backup_if_dirty` / `force`；前端在脏时弹确认框；失败自动重试 1 次。

**Tech Stack:** Rust (`skills`)、Tauri、React（`SkillsPanel` + 轻量确认对话框）、现有 Toast

**Spec:** [`docs/superpowers/specs/2026-07-15-skills-update-tab-design.md`](../specs/2026-07-15-skills-update-tab-design.md) v3 行

## Global Constraints

- **仅 v3**：不做三路 merge、不做后台静默更新。
- 有本地改动 → **必须先提示**；默认建议「备份并更新」，可选「直接覆盖」或「取消」。
- 备份目录：`~/.astro/skill-backups/{agent}/{folder}/{yyyyMMdd-HHmmss}/`（复制技能根目录）。
- 自动重试：**恰好 1 次**；仍失败则记入结果，不无限重试。
- TDD；隔离分支 `feat/skills-update-v3`；每 Task 单独 commit。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `skills/src/digest.rs` | 目录 content digest 计算 |
| Modify: `skills/src/models.rs` | origin 增加 `content_digest`；`SkillUpdatePreview` |
| Modify: `skills/src/origins.rs` / `install.rs` | 安装成功写 digest |
| Modify: `skills/src/update.rs` | preview / backup / force / retry |
| Modify: Tauri `skills_commands.rs` + `lib.rs` | `preview_skill_update`；扩展 update 参数 |
| Modify: `apps/desktop/src/types.ts` | Preview DTO |
| Modify: `SkillsPanel.tsx` + i18n + 少量 css | 确认流；批量时逐条或汇总确认 |
| Modify: spec 状态 → v3 已实现 |

---

### Task 1: content digest 纯函数

**Files:** `skills/src/digest.rs`, `lib.rs`, tests in digest.rs

```rust
/// 对技能根目录下常规文件计算稳定 digest（跳过 `.` 开头；路径用 `/`；按 path 排序后 hash）。
pub fn skill_content_digest(skill_root: &Path) -> Result<String>;
```

- [ ] RED：临时目录写两文件，digest 稳定；改一文件 digest 变；空目录有确定值
- [ ] GREEN → commit `feat(skills): skill directory content digest`

---

### Task 2: origin 存 digest + 安装写回

**Files:** `models.rs` (`content_digest: Option<String>`)、`install.rs` `record_after_install`、`origins`

安装/更新成功后：对 `agent_skills_dir/folder` 算 digest 写入 origin（失败仅 debug，不阻断安装）。

- [ ] 单测：upsert 后能找到 digest
- [ ] commit `feat(skills): persist content_digest on install`

---

### Task 3: preview_skill_update

```rust
pub struct SkillUpdatePreview {
    pub folder: String,
    pub has_local_changes: bool,
    pub has_baseline_digest: bool,
    pub current_digest: Option<String>,
    pub baseline_digest: Option<String>,
}

pub fn preview_skill_update(agent_id: Option<&str>, folder: &str) -> Result<SkillUpdatePreview>;
```

规则：无 baseline digest → `has_local_changes = false`，`has_baseline_digest = false`（未知，UI 仍展示覆盖警告文案，但不弹「检测到改动」强化框）。有 baseline 且 current ≠ baseline → true。

- [ ] 测试：相等 / 不等 / 无 baseline
- [ ] commit `feat(skills): preview local changes before skill update`

---

### Task 4: backup + update with force / retry

```rust
pub async fn update_installed_skill_ex(
    agent_id: Option<&str>,
    folder: &str,
    opts: UpdateSkillOpts, // { backup_if_dirty: bool, force: bool, max_retries: u8 = 1 }
) -> Result<String>;

fn backup_skill_dir(agent_id, folder) -> Result<PathBuf>; // 拷贝到 skill-backups/...
```

行为：
1. preview；若 dirty 且 `!force` → Err("本地有改动，请确认后 force 更新")
2. 若 dirty 且 `backup_if_dirty` → 先 backup
3. `install_from_ref`；失败则 sleep 短间隔（或立即）再试 1 次（`max_retries`）
4. 成功后刷新 digest（via record_after_install 路径）

保持旧 `update_installed_skill` 为 `force: true, backup_if_dirty: true` 以兼容，或改为走默认安全路径（**选定**：旧入口 = `backup_if_dirty: true, force: true`，避免破坏自动化；UI 新入口先 preview 再带用户选择调用）。

`update_outdated_skills` / `update_all`：对 dirty 项若未 force，跳过并记 `ok: false, message: 本地有改动…`；或批量前先返回 previews（UI 批量确认后 force）。**选定 v3 UI：** 单条弹窗；批量「全部更新」先 `preview` 全部 outdated，若任一条 dirty → 汇总确认「N 个有本地改动，将备份后更新」。

- [ ] 测试：backup 创建目录存在拷贝文件；dirty+!force Err；retry 测用可注入闭包或计数（若难则测 backup + preview 为主）
- [ ] commit `feat(skills): backup and retry on skill update`

---

### Task 5: Tauri 命令

- `preview_skill_update(folder, agentId?) -> SkillUpdatePreview`
- `update_installed_skill(folder, agentId?, force?, backupIfDirty?)` 扩展可选参数（默认 force=true backupIfDirty=true 保兼容）

- [ ] `cargo check -p astro-agent`
- [ ] commit `feat(tauri): preview and safer skill update options`

---

### Task 6: 前端确认 UX

**单条更新：**
1. `preview_skill_update`
2. 若 `has_local_changes` → 模态/确认：「检测到本地修改。备份并更新 / 直接覆盖 / 取消」
3. 调用 update，相应 `force` + `backupIfDirty`

**全部更新：**
1. 对 outdated folders 并行/串行 preview
2. 若有 dirty → 一句话确认备份并更新全部；取消则 abort
3. 逐条 update（force+backup）；失败已含 1 次重试；Toast 汇总

i18n：
```
skills.updateLocalChangesTitle
skills.updateLocalChangesBody
skills.updateBackupAndContinue
skills.updateOverwriteOnly
skills.updateCancel
skills.updateBatchLocalChanges // "{count} 个技能有本地修改，将先备份再更新"
skills.updateRetrying
```

复用现有对话框模式（若 Cron/Agent 有 drawer/confirm，跟项目一致；否则简单 `window.confirm` **不推荐** —— 用轻量 inline modal 或项目已有 Confirm 组件）。

- [ ] 实现 + tsc
- [ ] commit `feat(skills-ui): confirm before overwrite when skill locally modified`

---

### Task 7: 验收 + spec

- [ ] `cargo test -p skills digest:: update:: origins:: -- --test-threads=1`
- [ ] 前端相关 test / tsc
- [ ] spec 状态 → **v3 已实现**；链本 plan
- [ ] commit docs

---

## Spec coverage

| Spec v3 | Task |
|---------|------|
| 本地改动检测 | T1–T3 |
| 更新前明确提示 | T6 |
| 备份 | T4–T6 |
| 失败重试细化 | T4（1 次） |
| 三路 merge / 静默更新 | 不做 |

## Out of scope

- Git 三路合并
- 备份清理策略 UI（可只留文件在磁盘）
- 补录 install_ref
