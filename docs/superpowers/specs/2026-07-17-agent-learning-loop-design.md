# Agent 运行时学习闭环设计

**日期:** 2026-07-17  
**状态:** 已实现（P1 运行时）  
**范围:** Skills patch / curate、skill usage、learning 配置、prompt + 回合 nudge  
**外部参考（仅设计借鉴，不引入其品牌命名）：** 自我改进 Agent 的「create → maintain → evolve」闭环；evolve（离线遗传）为本规格 Phase 2。

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`learning`、`LearningConfig`、`skill usage`、`curate`、`pending_learning_nudge`、`manage_action=patch`。

## 目标

1. Agent 能在对话中 **创建 / 精准 patch / 删除** Agent 工作区 Skills  
2. **Curator**：基于加载时间给出闲置建议，**不自动删除**  
3. **Nudge**：复杂任务后一回合注入短提示；固定 guidance 说明何时用 skills / memory  
4. 与现有 Memory review / DecisionLog 衔接，不重复造第二套记忆  

## 明确不做（P1）

- Done 后 auxiliary 自动写 Skill  
- 离线 GEPA / DSPy 遗传优化仓  
- 自动 delete / disable skill  
- 拆分 `file_ops` / 新增独立 toolset id  
- 结构化 EntityMemory / Honcho 式用户建模  

## 决策摘要

| 项 | 选择 |
|----|------|
| Skill 写入范围 | 仅 `agent_skills_dir`（Agent `workspace/skills/`） |
| 局部修改 | `manage_action=patch`，唯一 `old_string` |
| 自动提炼 | 否；靠工具 + nudge |
| Curator | `action=curate` 只建议 |
| 配置段 | `config.yaml` → `learning:` |
| Usage 路径 | `~/.astro/learning/skill-usage.json` |

## 架构

```text
config.yaml (learning.*)
        │
        ▼
 AgentLoop ── begin_user_turn ──► pending_learning_nudge
        │                              │
        │                              ▼
        │                    build_system_prompt (guidance + dynamic)
        ▼
 skills 工具 ── load ──► record skill-usage.json
             ── curate ──► 闲置建议
             ── manage patch/create/update/delete
```

## 配置

见 [`docs/learning-loop.md`](../../learning-loop.md)。默认：`nudge_enabled=true`，`complex_task_tool_threshold=5`，`unused_skill_days=30`。

## 安全

- `skill_id`：禁止路径分隔与 `..`；仅 `[A-Za-z0-9._-]`  
- delete/patch/update：canonical 路径必须落在 Agent skills 根下  
- Curator 输出仅为建议文案  

## Phase 2

离线进化：轨迹 → 变体评测 → 门禁 → 人工 PR。另开规格与 crate，不阻塞 P1。

## 验收

- [x] `docs/learning-loop.md` + 本规格  
- [x] `manage patch` 单测（0/1/多命中）  
- [x] `load` 写 usage；`curate` 标闲置  
- [x] `learning` 配置加载；复杂回合后下一轮 guidance/dynamic 含 nudge  
- [x] 无用户可见 `hermes` 字样  

**状态:** 已实现（P1 运行时）  
