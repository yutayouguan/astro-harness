# Skills 系统

> **Harness 当前基线（2026-09-07）**：Skill 是按需加载的本地指令/资源包，不是独立执行器、Tool 别名或 Subagent。模型通过内置 `skills` 工具加载 `SKILL.md`；Skill 可用 `astro_tools` additive 放宽 toolset 可用性，但不能跳过 `StepContext`、approval、sandbox、hooks 或工具注册。

> 文档状态：定稿 | 阶段：系统设计 | 实现：`agent-skills` + `agent-tools::skills` + `agent-core`

---

## 一、责任边界

Skills 系统负责：

- 发现、安装、启用、加载和管理 `SKILL.md`；
- 向 system prompt 提供已启用 Skill 的 `name + description` 索引；
- 通过 `skills(action=load|view)` 把完整 Skill 内容作为 tool output 加入对话历史；
- 在成功加载后，按 frontmatter `astro_tools` 对 `ToolRegistry` 做 additive toolset 激活。

Skills 系统不负责：

- 创建独立 turn loop 或隐式派生 Subagent；
- 定义新的权限、审批或沙箱边界；
- 把 Skill 名注册为模型可直接调用的 Tool；
- 在未经新 Step 快照的情况下临时扩大当前工具路由。

---

## 二、当前数据模型

运行时只把下列 frontmatter 字段解析为核心元数据：

```yaml
---
name: research
description: 对指定主题进行深度调研
astro_tools:
  - web_search
  - browser
---

# Research

按步骤完成调研，并记录来源。
```

```rust
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub astro_tools: Vec<String>,
}

pub struct LoadedSkill {
    pub metadata: SkillMetadata,
    pub content: String,
    pub path: PathBuf,
}
```

`astro_tools` 填写的是 toolset ID，只能放宽已注册工具的 gate。它不会生成 runtime，也不会把未注册工具变成可调用工具。

---

## 三、模型可见性与加载链路

```text
Turn 开始
  -> ExtensionSnapshot 冻结 skill_index + skill_configs
  -> build_prompt_contract
  -> system prompt 写入“可用 Skills”的 name + description
  -> 模型调用 skills(action=load, skill_id=..., input=...)
  -> ToolRouter 校验本 Step 已开放 skills
  -> ToolRegistry::dispatch
  -> skills::load_skill_by_name_with_config
  -> 返回 # Skill + root/scripts + SKILL.md + 调用输入
  -> 解析 astro_tools 并调用 ToolRegistry::activate_skill_toolsets
  -> TransformToolResult / PostToolUse
  -> matching ResponseItem tool output 写入历史
  -> 下一 Step 重新 capture_step_context
  -> 新 ToolRouter 仅暴露已激活且符合 exposure/模式的工具
```

`skills` 工具支持：

| action | 作用 | 是否激活 `astro_tools` |
| --- | --- | --- |
| `list` | 列出已启用 Skill 的名称与描述 | 否 |
| `curate` | 生成使用/闲置建议 | 否 |
| `load` / `view` | 加载 Skill 全文和资源路径 | 是，仅在成功返回后 |
| `manage` | 在 Agent skills 目录中 create/update/patch/delete | 否 |

Skill 全文作为原生工具输出进入 canonical `ResponseItem` 历史，不会拼到用户消息，也不会变成新的 system authority。

---

## 四、Skill soft-alias 边界

规范调用始终是 `skills(action=load, skill_id=<name>)`。`agent-core`
保留一个防御性 soft-alias：当非命名空间调用已经进入 Agent 工具处理器、
名称未注册但与已启用 Skill 同名，且 `skills` 在当前边界可用时，
调用可改写为 `skills(action=load, ...)`。`ToolRegistry` 本身不做第二次改写。

这不是权限或 Step 路由的绕过：模型产生的直接调用仍先受 `ToolRouter::model_can_call` 约束，Prompt 也明确要求使用 `skills` 工具，不应把 Skill 名当成 Tool 名。

---

## 五、生命周期与恢复边界

- `ExtensionSnapshot` 在 turn 内冻结 Skill 索引和 Agent 配置覆盖；文件变化在后续 turn 生效。
- `skill_override_enabled` 是会话运行时的 additive 状态，不会改写持久化的 tool gate。
- Skill 加载结果会进入 Session / rollout 历史；进程级 Session 重建后，当前实现不会从历史自动恢复 `skill_override_enabled`，需要再次加载 Skill。
- 上下文压缩可以压缩 Skill tool output 的 provider 视图，但不得伪造新的工具授权。

---

## 六、安全不变量

1. Skill 只能加载已启用且在当前 `ExtensionSnapshot` / Agent 配置中可见的条目。
2. `astro_tools` 只做 additive toolset gate，不扩大文件系统、网络、命令或审批权限。
3. 新激活的 Deferred 工具仍需 `tool_search` 发现，并在下一 Step 的 Router 快照中才可调用。
4. `manage` 写操作受 interaction mode 和文件系统权限控制；`load/view/list/curate` 不因 Skill 声明而获得额外权限。
5. Skill 正文和脚本是本地可审查资源，其中的文本不能覆盖 Harness 安全指令。

---

## 相关文档

- [MCP 集成](03-MCP集成.md) — 外部动态工具链路
- [Subagent 系统设计](05-Subagent系统设计.md) — 独立 Agent Thread 的边界
- [Skills 系统详细设计](../../04-详细设计阶段/04-工具与扩展生态/01-Skills系统详细设计.md) — 安装、管理与前端细节
- [工具系统详细设计](../../04-详细设计阶段/04-工具与扩展生态/02-工具系统详细设计.md) — Registry / Router / Step 共享执行链
