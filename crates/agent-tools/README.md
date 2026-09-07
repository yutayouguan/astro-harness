# tools

Astro Agent 全部内置工具的实现、注册表、分发引擎与审批逻辑，通过 `inventory` 自注册机制实现零耦合工具扩展。

## 核心职责

- 提供 `CoreToolRuntime / ToolExecutor` 类型擦除执行合同，并由 `ToolRegistry` 统一持有内置、MCP 与动态工具的 schema 和 runtime
- 通过 `inventory` crate 实现工具自注册：新工具只需 `submit_builtin_tool!` 宏即可，无需修改入口
- `register_all()` 一次性收集并注册全部内置工具
- Agent 主链和独立测试统一走 `ToolRegistry::dispatch -> CoreToolRuntime::handle`，无平行 handler table
- 命令审批系统：危险命令分类、allowlist 匹配、hardline 阻断
- 交互模式门禁：按 `InteractionMode`（Agent/Plan）过滤可用工具 schema
- 工具目录与 schema 清洗：`builtin_catalog` / `sanitize_tool_schema`
- 路径安全：`resolve_safe` 防止路径穿越
- 沙箱集成：`SandboxAuditMetadata` 审计元数据

## 模块结构

| 文件/目录 | 职责 |
|-----------|------|
| **engine/** | |
| `engine/registry.rs` | `ToolRegistry` — 工具注册表（内置 + MCP + 动态），持有 schema、`CoreToolRuntime`、toolset 与暴露状态 |
| `engine/dispatch.rs` | Registry runtime 分发的统一记账、权限与 Skill soft-alias 逻辑 |
| `engine/context.rs` | `ToolContext` — 工具执行上下文（凭证、会话、沙箱策略、项目根） |
| `engine/catalog.rs` | `builtin_catalog` / `catalog_for_ui` — 工具目录与参数提取 |
| `engine/schema.rs` | `sanitize_tool_schema` — schema 清洗（移除厂商扩展、修复 hazards） |
| `engine/path_safe.rs` | `resolve_safe` — 路径安全解析，防止路径穿越 |
| `engine/execution.rs` | `AgentThreadDispatch` trait — 子 Agent 线程分发接口 |
| `engine/executor.rs` | `CoreToolRuntime / ToolExecutor` — 类型擦除执行合同与动态工具运行时 |
| `engine/network.rs` | `InProcessNetworkGrant` — 进程内网络访问授权 |
| **builtin/shell/** | |
| `builtin/shell/terminal.rs` | `terminal` 工具 — 沙箱化 shell 命令执行 |
| `builtin/shell/file_ops.rs` | `file_ops` 工具 — 文件读写/删除/列表/mkdir/patch |
| `builtin/shell/code_exec.rs` | `code_exec` 工具 — 代码执行（Python/Node/Rust） |
| `builtin/shell/web_search.rs` | `web_search` 工具 — 网页搜索 |
| `builtin/shell/web_fetch.rs` | `web_fetch` 工具 — URL 内容获取 |
| `builtin/shell/jobs.rs` | 后台任务管理：`shutdown_all_jobs` / `shutdown_jobs_for_session` |
| **builtin/media/** | |
| `builtin/media/image_gen.rs` | `image_gen` 工具 — 图像生成 |
| `builtin/media/tts.rs` | `speech_gen` 工具 — 文本转语音 |
| `builtin/media/music_gen.rs` | `music_gen` 工具 — 音乐生成 |
| `builtin/media/video_gen.rs` | `video_gen` 工具 — 视频生成 |
| `builtin/media/image_understand.rs` | 图像理解 |
| `builtin/media/audio_understand.rs` | 音频理解 |
| `builtin/media/video_understand.rs` | 视频理解 |
| `builtin/media/robotics.rs` | 机器人控制 |
| **builtin/memory/** | |
| `builtin/memory/memory_tools.rs` | `memory` 工具 — 记忆增删改查 |
| `builtin/memory/context_tools.rs` | 上下文固定工具 |
| `builtin/memory/skills_tool.rs` | `skills` 工具 — Skill 加载/管理 |
| `builtin/memory/scheduled.rs` | `scheduled` 工具 — Cron 定时任务管理 |
| `builtin/memory/todo.rs` | `todo` 工具 — 待办事项 |
| **builtin/agents/** | |
| `builtin/agents/subagent.rs` | Agent 工具集：`spawn_agent` / `list_agents` / `send_message` / `followup_task` / `wait_agent` / `interrupt_agent` |
| `builtin/agents/persona_create.rs` | `persona_create` — Agent 人格创建 |
| **builtin/hitl/** | |
| `builtin/hitl/ask_user.rs` | `ask_user` 工具 — 向用户提问 |
| `builtin/hitl/switch_mode.rs` | `switch_mode` 工具 — 切换交互模式 |
| **builtin/present/** | |
| `builtin/present/tool.rs` | `present` 工具 — AG-UI 组件呈现 |
| `builtin/present/present_shared.rs` | 呈现工具共享逻辑 |
| **其他** | |
| `approval.rs` | 命令审批：`classify_dangerous_command` / `is_hardline_blocked` / `matches_allowlist` |
| `interaction_mode.rs` | `filter_schemas` / `tool_visible_in_mode` — 交互模式工具过滤 |

## 核心类型与 API

- `ToolRegistry` — 工具注册表：`register_runtime()` / `register_dynamic()` / `available_tools()` / `schemas_for_api()` / `dispatch()`；Deferred namespace 由 `tool_search` 返回完整子工具 schema
- `ToolCatalogItem` — 前端目录 DTO：`id` 是 toolset，`name` 是模型调用名，`namespace` 与 `registeredName` 显式区分协议身份和内部 handler
- `register_workflow_tools()` — 每 Step 从 WorkflowStore 快照生成 `workflow` namespace、执行 runtime 和加载策略；默认 Deferred 且禁止在无 `tool_search` 模型上 eager 降级
- `ToolContext` — 工具执行上下文：凭证、会话 ID、沙箱策略与项目根；延迟工具搜索只能通过只读 `ToolRegistryView` 访问目录
- `register_all(registry)` — 一次性注册全部内置工具（通过 `inventory` 自动收集）
- `ToolRegistry::dispatch(ctx, name, args)` — Agent 规范分发入口
- `submit_builtin_tool!` — 宏：为内置工具生成原生 `ToolExecutor`，并将元数据与 runtime 一起注册到 `inventory`
- `AgentThreadDispatch` — trait：子 Agent 线程分发（spawn/followup/interrupt/wait）
- `CoreToolRuntime / ToolExecutor` — trait：Registry 持有的执行运行时与工具实现合同
- `classify_dangerous_command(cmd)` — 危险命令分类
- `filter_schemas(mode, schemas)` — 按交互模式过滤工具 schema
- `sanitize_tool_schema(schema)` — 移除厂商特定字段、修复 schema hazards
- `resolve_safe(base, path)` — 安全路径解析

## Crate 关系

| 方向 | crate | 说明 |
|------|-------|------|
| 依赖 | `types` | ToolEntry、ToolOutput、ToolCallAccumulator、审批类型 |
| 依赖 | `memory` | 记忆工具调用 MemoryManager |
| 依赖 | `sandbox` | 沙箱策略与审计 |
| 依赖 | `session` | 会话库访问 |
| 依赖 | `home` | 路径约定、agent config |
| 依赖 | `providers` | 媒体生成（image_gen/tts/music/video） |
| 依赖 | `skills` | Skill 加载与管理 |
| 依赖 | `subagents` | 子 Agent 生命周期 |
| 依赖 | `cron` | Cron job 管理 |
| 依赖 | `a2ui` | AG-UI 组件呈现 |
| 依赖 | `artifacts` | 文件空间索引 |
| 依赖 | `hooks` | Hook 集成 |
| 依赖 | `usage` | 用量追踪 |
| 被依赖 | `agent`（agent-core） | 运行时通过本 crate 注册与分发工具 |

## 关键不变量

1. **inventory 自注册**：新工具无需修改 `register_all`，只需 `submit_builtin_tool!` 宏声明即可
2. **Runtime 完备性**：每个注册的 metadata 工具必须有对应 `CoreToolRuntime`（由测试强制）
3. **schema 清洗**：所有工具 schema 在发往 LLM 前必须经过 `sanitize_tool_schema` 处理
4. **路径安全**：文件操作工具必须通过 `resolve_safe` 验证路径不穿越工作区
5. **Runtime 快照**：模型可见 schema 必须存在同身份 `CoreToolRuntime`；替换 metadata 会立即使旧 Runtime 失效
6. **规范身份唯一**：两个 registered name 不得投影到同一 `(namespace, name)`
7. **交互模式门禁**：Plan 模式下写类工具不可见；`filter_schemas` 负责过滤
8. **只读发现**：`tool_search` 仅依赖 `ToolRegistryView`，不持有可变注册表能力

## 测试

```bash
cargo test -p tools
```
