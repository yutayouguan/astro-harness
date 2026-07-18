# Astro Agent 学习闭环

Astro 将「可复用工作流」固化为 **Skills（程序性记忆）**，将「长期偏好/事实」写入 **Memory**，并在对话中用 **nudge** 提醒 Agent 何时沉淀经验。完整学习闭环分三层：

| 层 | 状态 | 说明 |
|----|------|------|
| **运行时闭环** | 本期已落地 | Agent 用 `skills` / `memory` 建改；回合后复杂任务 nudge；`curate` 修剪建议 |
| **记忆 review** | 已有 | 回合后 `auxiliary.background_review` 精炼 MEMORY/USER（见 [`memory.md`](./memory.md)） |
| **离线进化** | Phase 2（Rust 内置极简版已落地） | 读轨迹→反思模型提技能候选→门禁→应用内待审批；完整 GEPA 遗传搜索仍为后续 |

设计规格：[`docs/superpowers/specs/2026-07-17-agent-learning-loop-design.md`](./superpowers/specs/2026-07-17-agent-learning-loop-design.md)。

---

## 运行时流程

```text
复杂任务成功 / 纠错找到通路
        │
        ▼
  learning nudge（下一轮 system）
        │
   ┌────┴────┐
   ▼         ▼
 skills    memory
 create/   add/replace
 patch
        │
        ▼
 skills curate（建议 disable/delete，不自动删）
```

### 何时沉淀 Skill

- 本轮工具调用次数 ≥ `learning.complex_task_tool_threshold`（默认 5）且任务成功  
- 踩坑后找到正确路径，应 `manage_action=patch` 写回已有 Skill  
- 用户纠正了步骤或发现可复用工作流  

持久偏好、环境事实走 **`memory`**，不要写进 Skill 正文。

### 工具契约（`skills`）

| action | 作用 |
|--------|------|
| `list` | 已启用 name + description |
| `load` / `view`（默认） | 加载 SKILL.md + 路径/scripts |
| `curate` | 启用列表 + 上次加载时间 + 闲置建议 |
| `manage` | `create` / `update` / `patch` / `delete`（仅 Agent `workspace/skills/`） |

`patch`：`old_string` 须在 SKILL.md 中**唯一**匹配，再替换为 `new_string`（对齐 `file_ops.patch`）。

---

## 配置

路径：`~/.astro/config.yaml` 的 `learning:` 段。

```yaml
learning:
  nudge_enabled: true                 # 复杂任务后是否在下一轮注入提示
  complex_task_tool_threshold: 5      # 本轮工具次数达到该值视为「复杂」
  unused_skill_days: 30               # curate 将更久未加载的技能标为闲置建议
```

| 键 | 默认 | 说明 |
|----|------|------|
| `nudge_enabled` | `true` | 关闭后不注入复杂任务 / DecisionLog 失败强化提示 |
| `complex_task_tool_threshold` | `5` | 上一轮工具轮次 ≥ 该值则下一轮挂起学习 nudge |
| `unused_skill_days` | `30` | `curate` 建议阈值 |

使用统计文件：`~/.astro/learning/skill-usage.json`（`load` 成功时更新 `last_loaded_at`）。

---

## 与记忆系统的关系

- **MEMORY / USER**：有界事实与偏好；可选 `write_approval`、background review、入梦。见 [`memory.md`](./memory.md)。  
- **Skills**：可执行流程与脚本索引；Agent 主动 `manage`，本系统**不**在 Done 后自动写 Skill。  
- **DecisionLog**：`~/.astro/learning/decisions.jsonl`；近期 `ToolFailure` 可强化「把正确路径 patch 进 skill」提示。  
- **session_search**：跨会话原文召回；摘要仍由模型完成，不自动入库。

---

## Phase 2（离线进化）

离线遗传优化：读取执行轨迹 → 生成 Skill/提示变体 → 测试与体积门禁 → 人工审 PR。独立流水线，不改变运行时默认行为。

**已落地（Rust 内置极简引擎）**：模型服务页「离线进化」子 Tab 可配置 `evolution.enabled`、`reflection` / `judge` 路由与门禁（run_tests / require_pr / max_skill_bytes，写入 `config.yaml` 的 `evolution:` 段），并新增：

- **运行进化**：读 `learning/decisions.jsonl`（工具失败等）+ 已启用技能索引 → `reflection` 模型产出技能候选（新建 / patch）→ 静态门禁 → **`judge` 模型打分**（`min_judge_score` 阈值，0 关闭）→ 存待审提案（`~/.astro/learning/evolution/proposals/`）。
- **应用内审批**：提案在子 Tab 内以 diff + judge 评分展示，**批准**才写入 Agent skills 目录，**绝不自动应用**（`require_pr` 语义）。

实现：crate [`evolution`](../evolution)（candidate/reflect/gates/judge/proposal）+ Tauri `evolution_run_commands`（run/list/approve/reject）+ `EvolutionModelsPanel`。

**仍为后续**：完整 GEPA/Pareto 遗传搜索、`run_tests` 实跑（技能多为 Markdown，暂仅当存在 `scripts/test.*` 时由调用方执行）、git 分支/PR 自动化。

模型角色（不复用在线 `auxiliary.*`）：

| 角色 | 职责 | 模型倾向 |
|------|------|----------|
| reflection | 读 trace 诊断失败并提出改写 | 强推理模型 |
| judge | 对候选判分 | 可省或中等模型 |
| target | 被优化对象实际运行 | 生产主模型 |

详见规格 [`specs/2026-07-17-agent-learning-loop-design.md`](./superpowers/specs/2026-07-17-agent-learning-loop-design.md)。
