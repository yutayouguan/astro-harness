# Skills 系统详细设计

> **当前实现基线（2026-09-07）**：Skill 是 `SKILL.md` 指令/资源包。模型只通过内置 `skills` 工具加载 Skill；`astro_tools` 只对已注册 toolset 做 additive gate。Skill 不注册独立执行循环、不隐式派生 Subagent，也不能绕过 `ToolRouter`、approval、sandbox 或 hooks。

> 阶段：详细设计
> 状态：当前代码契约
> 对应模块：`crates/agent-skills`、`crates/agent-tools/src/builtin/memory/skills_tool.rs`、`crates/agent-core/src/runtime`

---

## 1. 模块职责

| 模块 | 职责 |
| --- | --- |
| `agent-skills::installed` | 扫描、去重、启用状态、frontmatter 解析和按名称加载 |
| `agent-skills::install/update/snapshots` | 安装、更新、备份与恢复 Skill 文件 |
| `agent-skills::usage` | 记录加载次数与生成 curate 建议 |
| `agent-tools::skills_tool` | 注册并执行 `skills` 的 `list/curate/load/view/manage` action |
| `agent-core::system_prompt` | 从 turn 快照生成已启用 Skill 的名称/描述索引 |
| `agent-core::tool_dispatch` | 规范化调用、成功加载后的 `astro_tools` 激活和统一结果处理 |
| `ToolRegistry / ToolRouter / StepContext` | gate、模型可见性和单 Step 执行边界 |

不存在 `Skill::execute`、`SkillRunner` 或 Skill 专用工具分发器。Skill 的动作最终都通过普通 `CoreToolRuntime` 链执行。

---

## 2. 存储与发现

### 2.1 Astro 管理范围

- 全局安装：`~/.astro/skills/<skill>/SKILL.md`
- Agent 工作区：`~/.astro/agents/<agent_id>/workspace/skills/<skill>/SKILL.md`
- Agent 工作区兼容发现目录：`.agents/skills`、`.cursor/skills`
- 可信项目安装：`<project>/.astro/skills/<skill>/SKILL.md`
- 编译进应用的 bundled Skills：只读，`scope=builtin`

外部 machine scope 可扫描 `.agents/.codex/.claude/.cursor/.astro` 的 Skills，但不会因为被扫描到就自动获得 Astro Agent 的写权限；链接和启用状态仍由 Astro 管理层决定。

### 2.2 当前核心数据结构

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

运行时核心解析器只消费 `name`、`description` 和 `astro_tools`。其它曾在旧设计中出现的 `allowed-tools`、`context: fork`、Skill Hooks、BM25 阈值等字段不是当前执行契约。

---

## 3. `skills` 工具协议

```rust
pub struct SkillsArgs {
    pub action: Option<String>,
    pub skill_id: Option<String>,
    pub manage_action: Option<String>,
    pub content: Option<String>,
    pub description: Option<String>,
    pub old_string: Option<String>,
    pub new_string: Option<String>,
    pub input: Option<serde_json::Value>,
}
```

| action | 必要字段 | 行为 | 激活 `astro_tools` |
| --- | --- | --- | --- |
| `list` | 无 | 返回已启用 Skill 的名称与描述 | 否 |
| `curate` | 无 | 返回使用统计和闲置建议，不自动删除 | 否 |
| `load` / `view` | `skill_id` | 读取 Skill 正文、root、scripts 和调用输入 | 是，成功后 |
| `manage` | `skill_id`, `manage_action` | create/update/patch/delete Agent 工作区 Skill | 否 |

`skill_id` 禁止路径分隔符和 `..`。`manage patch` 要求 `old_string` 唯一；更新外部只读来源时写入 Agent skills 目录，不原地覆盖外部来源。

---

## 4. Turn、Prompt 与加载链路

```text
begin turn
  -> ExtensionSnapshot freezes skill_index + skill_configs
  -> system_prompt_parts
  -> prompt contains enabled Skill name + description
  -> model calls skills(action=load|view, skill_id, input)
  -> ToolRouter::model_can_call("skills")
  -> approval / interaction-mode checks
  -> ToolRegistry::dispatch
  -> macro-generated native ToolExecutor
  -> skills_tool::dispatch
  -> load_skill_by_name_with_config
  -> ToolOutput("# Skill: ..." + root/scripts + SKILL.md + input)
  -> activate_skill_toolsets_from_args
  -> TransformToolResult / PostToolUse
  -> canonical matching ResponseItem output
  -> next capture_step_context
```

Skill 正文不直接改写用户消息，也不获得 system/developer authority。它作为工具结果进入原生历史，并受正常的 spill、压缩和 Provider 历史转换规则约束。

---

## 5. `astro_tools` 激活

成功执行 `load/view` 后，`agent-core` 使用同一 Step 冻结的 `skill_configs` 再次解析该 Skill 的 `astro_tools`，然后调用：

```rust
ToolRegistry::activate_skill_toolsets(&astro_tools);
```

约束如下：

1. 只处理成功的 `load/view`；`list`、`curate`、`manage` 不激活工具。
2. 不使用跨 Session 的“最近加载”全局缓存；同名 Skill 必须按当前 Agent/Step 配置解析。
3. 激活仅加入 `skill_override_enabled`，不会创建 `ToolEntry` 或 runtime。
4. 当前 Step 已冻结；新 gate 只影响下一次 `capture_step_context()`。
5. Direct 工具可在下一 Step 暴露；Deferred 工具仍需 `tool_search` 发现；Hidden 工具不暴露。
6. toolset 可用不等于调用获批，approval、sandbox、hooks 和 permission profile 继续生效。

---

## 6. Soft-alias

规范调用方式是：

```json
{
  "name": "skills",
  "arguments": {
    "action": "load",
    "skill_id": "research",
    "input": {}
  }
}
```

`agent-core::handle_tool_call_async_scoped` 保留一次防御性 soft-alias：非命名空间调用已经进入工具处理器、名称未注册但匹配已启用 Skill，且 `skills` 在当前边界可用时，才改写为上述形式。

`ToolRegistry::dispatch` 不执行第二次 soft-alias，也不接受未注册 runtime。模型来源的调用仍先经过 `ToolRouter::model_can_call`，因此不能靠猜测 Skill 名绕过本 Step 的模型可见集合。

---

## 7. 生命周期与恢复

| 状态 | 持有者 | 生命周期 |
| --- | --- | --- |
| Skill 索引与配置覆盖 | `ExtensionSnapshot` | 当前 turn 冻结 |
| Skill 正文 | matching tool output / Session / rollout | 按原生历史持久化 |
| `skill_override_enabled` | 当前 Session 的 `ToolRegistry` | 进程内会话状态 |
| 安装与启用状态 | Skills 目录和 `skills-enabled.json` | 持久化 |

当前限制：Session 在进程级重建后不会从历史 `# Skill:` 输出自动恢复 `skill_override_enabled`，因此需要再次调用 `skills(action=load|view)`。历史正文仍可保留在上下文中，但历史内容本身不能作为工具授权。

---

## 8. 与 MCP、Subagent 的边界

- Skill 不控制 MCP enablement；MCP 工具只由 Server/单工具配置、健康连接、Step 可见性和审批策略决定。`astro_tools: [mcp]` 不会额外放宽 MCP。
- Skill 不创建 MCP 连接，不保存 MCP 凭据，也不决定 MCP approval。
- Skill 不隐式派生 Subagent；需要独立 Agent Thread 时必须显式调用 `spawn_agent`。
- Subagent 使用自己的 Session、`ExtensionSnapshot` 和工具 Registry，Skill 配置只能在父权限上限内继承或收窄。

---

## 9. 测试要求

- frontmatter 的 block/inline `astro_tools` 解析一致；
- Agent 配置覆盖按当前路径解析，同名 Skill 不得跨 Session 串扰；
- 只有成功 `load/view` 激活 toolset；
- runtime 元数据与 `ToolEntry` 身份、描述、toolset、图标和审批要求一致；
- 新激活工具只在下一 Step 出现；Deferred 仍依赖 durable `ToolSearchOutput`；
- Skill 不能绕过 interaction mode、approval、sandbox 或 hooks。

---

## 10. 相关文档

- [Skills 系统设计](../../03-系统设计阶段/02-核心功能模块/04-Skills系统.md)
- [工具系统详细设计](02-工具系统详细设计.md)
- [MCP 协议详细设计](03-MCP协议详细设计.md)
- [Responses API 原生工具协议与 Astro 工具协议](05-Responses-API原生工具协议与Astro工具协议详细设计.md)
- [Subagent 详细设计](../01-核心引擎层/06-Subagent详细设计.md)
