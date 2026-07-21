---
name: create-agent
description: 根据用户填写的助手模板，创建**持久** Agent 记忆空间（workspace-{id}）、写入 agents/{id}/config.json，并填充 AGENT/IDENTITY/SOUL/USER/MEMORY 等 md。用户说「帮我创建一个助手」或点击「新建 Agent」时使用。不要用于回合内委派子任务——那是 delegate 工具。
astro_bundled_rev: 4
---

# 创建持久 Agent（记忆空间）

当用户用下面模板（或等价描述）要求**新建长期助手人格**时，执行本技能。

**这与 `delegate` 不同**：`agent_create` 新建可切换、带 MEMORY/IDENTITY 的持久助手；`delegate` 仅在当前回合 spawn 短暂子任务（不建 workspace、不写 MEMORY）。不要把「委派一个子任务」说成或做成「创建一个 Agent」。

## 用户模板

```
帮我创建一个助手：名称是「」，背景经历是「」，说话风格是「」，主要帮我做「」，不要做「」，请称呼我为「」，我的偏好是「」
```

## 目录约定

| 路径 | 用途 |
|------|------|
| `~/.astro/workspace/` | 默认 Agent 工作区（固定 id=`workspace`） |
| `~/.astro/workspace-{id}/` | 其他 Agent 工作区；新建 id 为 `{slug}--{hex12}`（如 `ppt-expert--a1b2c3d4e5f6`） |
| `~/.astro/agents/{id}/config.json` | 该 Agent 的模型 / 工具 / MCP 配置 |
| `~/.astro/skills/` | 公共技能（本技能所在） |
| `workspace-{id}/skills/` | 该 Agent 专属技能 |

## 步骤

1. **解析**模板里「」中的字段；空字段可追问，或先用合理默认再写入。
2. **调用工具** `agent_create`，传入：
   - `name`：显示名（可中文、可与已有助手重名）
   - **不要传 `id`**（除非用户明确指定）；系统自动生成不可变 `{slug}--{hex12}`（slug 取自 name 的 ASCII 快照，纯中文则用 `agent`）
   - `activate`: false（默认不切换；需要立刻用新 Agent 时再传 true）
   - `inherit_config`: true（默认继承全局工具/MCP，可再改）
   - `profile`：background / style / focus / avoid / call_me / preferences
3. 工具会创建 `workspace-{id}/` 与 `agents/{id}/config.json`，并按 profile 填充各 md。
4. **图标**：无需手工指定。若创建引导页未上传图标，系统会按名称 / 背景 / 职能自动挑选 Lucide 图标写入 `assets/emoji.svg`。
5. 若还需微调，用 `file_ops` 编辑对应 md（不要改错工作区）。
6. 用一两句话告诉用户：新 Agent 的**显示名**、**id**、工作区路径、已选图标，以及是否已切换为当前 Agent。

## 注意

- 不要覆盖默认 `workspace`。
- `id` 与显示名解耦：改名不改目录 / cron 绑定；slug 只是创建时的可读快照。
- 配置（模型/工具/MCP）写在 `agents/{id}/`，工作区文件写在 `workspace-{id}/`。
- 专属技能放 `workspace-{id}/skills/`；公共技能继续用 `~/.astro/skills/`。
- 不必为图标额外调用工具；`agent_create` 会自动完成。
- 并行改代码仓、拆解当前任务 → 用 `delegate`（可选 git worktree），不要 `agent_create`。
