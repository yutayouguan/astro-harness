# Agent Harness 层项目建议书（本地优先 · 从 0 到 1）

> 状态：**建议 / 目标设计**（含「当前基线」对照）
>
> 日期：2026-09-16
>
> 适用仓库：Astro Agent（Rust workspace + Tauri 2 + React/Vite）
>
> 定位：本文是能力建议与演进路线，不是运行时契约。与
> [Agent Harness 总体架构](../03-系统设计阶段/01-架构设计/11-Agent-Harness总体架构.md)
> 及当前源码冲突时，以后者为准。文中每条建议都标注了「现状锚点」与「验收方式」，便于直接拆成任务。

---

## 0. 一页摘要

**一句话定位**：在模型之外构建一层本地优先的 Harness，把 LLM 的意图转化为
**可预算、可审批、可恢复、可审计**的真实执行。

**统一定义**：**Agent = Model + Harness**。Model 只负责推理与工具意图；
Harness 负责 Reason → Act → Observe 循环、上下文、工具运行时、权限、安全、
恢复、观测与运行环境。

**五个不能被替代的支点**（差异化来源，其余能力都挂在这五个支点上）：

| 支点 | 一句话 | 关键机制 |
| --- | --- | --- |
| 能力热插拔 | 运行中长出新能力，不重启、不重编排 | 每轮 `reload_tools_and_mcp()` + Step 快照冻结 + Skill `astro_tools` 增量授权 |
| 延迟工具发现 | 模型不背全量 schema，按需检索 | 原生 `tool_search`（BM25）+ `ToolSearchOutput` 授权 + 命名空间 canonical 路由 + Deferred 五级暴露 |
| 预算化上下文 | 上下文是资源，不是垃圾桶 | prune → 辅模型摘要 → head/tail 三段压缩 + 原文/模型视图分离 + 大结果 spill |
| 尝试级安全 | 审批、沙箱、网络、Hook 绑定同一次执行尝试 | `StepContext` 不变量 + HITL/smart approval/trust 分级 + attempt-scoped network lease |
| 事件溯源 | 事件流是事实源，SQLite 只是投影 | append-only rollout + 每 Thread live boundary + 崩溃重建 + 逐 Agent/session/tool 归因 |

**本地优先的三个硬约束**（决定架构取舍，不是营销词）：

1. **数据不出机**：对话、rollout、记忆、审计只落本机 `~/.astro`；Provider 是唯一外呼面。
2. **能力可换**：多 Provider + 多后端，Harness 不把语义焊死在某家 API 上（Agent 路径统一走 Responses）。
3. **离线可信**：没有云端编排服务，恢复与审计必须由本地事件流自证，而不是靠远端状态。

---

## 1. 项目定位与命题

### 1.1 要解决的问题

裸模型调用只能解决「说」，解决不了「做」。一旦要让模型真的动文件、跑命令、连 MCP、
开浏览器、起子任务，就必然面对六个工程问题：

1. 多步循环由谁拥有（模型返回工具调用之后会发生什么）；
2. 上下文预算由谁裁决（历史、工具结果、记忆、召回如何分配 Token）；
3. 工具边界由谁定义（模型可见 ≠ 可执行 ≠ 已授权）；
4. 危险动作由谁批准（谁在什么粒度上承担风险）；
5. 中断与崩溃后由谁重建（副作用不能靠猜）；
6. 事后由谁举证（谁在什么时候用哪把钥匙做了什么）。

这六问的答案共同体，就是 Harness。

### 1.2 设计命题

| 命题 | 反面（本项目拒绝的形态） |
| --- | --- |
| 执行是**事件驱动、可恢复**的 | 把副作用绑在「最后一条消息」上，崩溃即重放 |
| 工具边界是**快照化**的 | 热加载随时改写已经发出的调用语义 |
| 授权是**尝试级**的 | 一次性全局开关，或审批与执行脱钩 |
| 上下文是**预算化**的 | 只做截断，或只保留摘要而丢原文 |
| 状态是**可举证**的 | 只有 UI 现象，没有 rollup 级证据链 |

### 1.3 非目标（本阶段明确不做）

- 不做云端托管 Agent 平台与多租户编排服务；
- 不做 Remote Extension Marketplace 的信任根与计费（依赖自有服务与认证，当前仅设计审查）；
- 不把「子 Agent 必然拥有独立 worktree」当作运行方案；
- 不为兼容旧协议保留别名与回退路径（旧形态按迁移规则处理）。

---

## 2. 总体架构

### 2.1 分层

```text
入口层    Desktop(React/Tauri)  |  gRPC 客户端  |  Cron  |  Subagent runner
                            |
协议层    agent-protocol::{Op, EventMsg, TurnItem, ResponseItem}
                            |
会话层    AstroThread  →  Session  →  submission_loop  →  SessionTask/RegularTask
                            |
回合层    TurnContext  →  prepare_turn  →  PromptContract（稳定指令 + 动态上下文）
                            |
步骤层    StepContext（工具/路由/配置快照）  →  Responses 流式请求
                            |
执行层    ToolAccumulator → ToolRouter → 审批 → 沙箱/网络 → Hooks → handler/MCP
                            |
事实层    rollout（append-only）  →  SQLite 投影  →  live listener（gRPC/Tauri）
```

关键点：**上边界是 `Op`，下边界是真实执行环境，对外观察边界是 `EventMsg`/`TurnItem`**。
任何绕过 `Op` 直接注入历史的路径、任何绕过 `ToolRouter` 直接执行 handler 的路径，
都视为架构违规。

### 2.2 运行层级与不变量

| 层级 | 对象 | 生命周期 | 关键不变量 |
| --- | --- | --- | --- |
| Thread | `AstroThread` | 会话级长生命周期 | 单 submission loop；I/O 只绑定一次 |
| Session | `Session` | 会话共享状态与服务 | 单活跃 turn；录取与事件分发串行 |
| Task | `SessionTask` / `RegularTask` | 一次可取消工作 | 安装新任务前必须中止旧任务 |
| Turn | `TurnContext` | 一条用户意图 | `tool_rounds` 归零；输入准入可开关 |
| Step | `StepContext` | 一次采样及其工具执行 | 工具/路由/配置快照不被热加载突变 |
| Attempt | tool execution attempt | 一次真实执行尝试 | 审批与网络 lease 只属于本次尝试 |

### 2.3 唯一事实源

```text
reduce state → persist rollout → deliver live event
```

- Core 生成 `EventMsg`，先按策略写 rollout，再由 Server 每 Thread listener 投影；
- `RolloutItem` 现有 9 类：`SessionMeta`/`ResponseItem`/`RealtimeItem`/`TokenUsage`/`EventMsg`/`TurnContext`/`WorldState`/`Compacted`/`InterAgentCommunication`；
- 重启与订阅恢复使用 **rollout snapshot + live boundary**，历史与 live 通过稳定 item/turn identity 去重；
- SQLite（`state.db`，schema v24，WAL + FTS5）是**查询投影**，不是执行事件源；
- 恢复**不得**通过「猜最后一条消息」重放副作用。

---

## 3. 必答能力设计

每项统一按「目标 / 机制 / 不变量 / 现状锚点 / 建议补强」展开。

### 3.1 执行引擎：submit → reason → act → observe

**目标**：单活跃任务闭环，任何时刻只有一个 turn 拥有执行权，其余提交进入队列或被中断。

**机制**

```text
Op(UserInput/Steer/Interrupt/SettingsUpdate)
  → submission_loop（串行录取）
  → abort-old → install → bind → start（task_admission 串行化）
  → prepare_turn：持久化输入 → 热加载工具/MCP → 构建 PromptContract → 捕获 Step 快照
  → 采样（流式）→ ToolAccumulator 累积原生 tool_call_delta
  → Act：审批 → 沙箱 → Hooks → handler/MCP
  → Observe：结构化结果先落盘，再回灌
  → 无工具调用 → 最终文本 → 结束
```

**不变量**

- `ToolCallAccumulator` 只累积原生 tool_call delta，自由文本不参与工具识别；
- 模型的 assistant/tool item 必须先写入 `response_items`，再执行工具并写 matching output；
- 停止条件必须是显式的：最终文本 / 用户取消 / 预算耗尽 / 等待人类决策 / Provider 链失败 / 不可恢复错误。

**现状锚点**：`agent-core/src/runtime/{astro_thread,submission_loop,turn_lifecycle}.rs`、
`streaming/multi_turn.rs`、`streaming/tools_exec.rs`。

**建议补强**

- P1：为「批量工具并发」显式定义**并行批次**语义——哪些工具可并行、并发上限、
  同批内取消传播，并把批次 id 写入 rollout，使回放能重建「并发发生但持久化有序」的事实。
- P1：为 abort-old 引入**取消原因枚举**（用户中断 / 新提交覆盖 / 预算耗尽 / 关停），
  当前取消语义在审计上只有「中断」，缺少归因。

### 3.2 双层预算：turn / thread

**目标**：预算不是单一计数器，而是「单条用户意图」与「整个线程」两层约束叠加。

**机制**

| 预算 | 作用域 | 表现 |
| --- | --- | --- |
| tool depth | 单条用户消息 | `begin_user_turn()` 归零，`multi_turn`（默认 90）为上限，`increment_tool_round()` 超限返回 `MaxDepthError` |
| token / context | 每 Step | `ContextUsage` 混合快照（provider reported > recomputed > local estimate）+ 本地 segments 解释 |
| cost / usage | Thread | `TokenUsageRecord.latest` / `cumulative` / `compaction_response_id`；fork 不继承累计值 |

**不变量**：turn aggregate usage 用于计费与单轮统计；latest sampling usage 用于校准上下文环，
两者不可混用；`reasoning_tokens` 是 output 子集，`cached_input_tokens` 是 input 子集，均不重复计入总量。

**现状锚点**：`agent-core/src/runtime/budget*.rs`、`prompt/context_usage.rs`、
`agent-rollout` 的 `TokenUsage` 记录、`agent-usage`。

**建议补强**

- P1：把「预算将尽」提前暴露成事件（`BudgetPressure`），让 UI 与模型都能在耗尽前收敛，
  而不是在第 90 轮硬失败。
- P1：thread 级预算目前偏重 token/费用，建议补 **wall-clock 与工具调用次数**两个维度，
  否则长跑任务只能靠超时兜底。

### 3.3 工具运行时与热加载（必答项 ★）

**目标**：模型可见性、可执行性、授权三者解耦；运行中新增/移除能力**不需要重启**，
且**不得**改变已经发出工具的调用边界。

**机制：三层解耦**

| 维度 | 取值 | 语义 |
| --- | --- | --- |
| `ToolExposure` | Direct / DirectModelOnly / Deferred / DeferredModelOnly / Hidden | 是否直接出现在模型 schema 中 |
| `ToolMode` | Direct / CodeMode / CodeModeOnly | 以工具直调还是以控制面（`exec`/`wait`）编排 |
| 授权 | approval / sandbox / hooks / interaction mode | 与暴露正交，单独裁决 |

**机制：热加载路径**

```text
每轮 turn 开始：Session::reload_tools_and_mcp()
  ├─ reload_tool_gates()          # config.toml 的 tool gate
  ├─ reload_workflow_tools()      # WorkflowStore → workflow namespace
  ├─ set_extension_toolsets()     # turn-frozen ExtensionSnapshot
  └─ reload_mcp_from_snapshot()   # MCP Hub → ToolRegistry（先卸 MCP_TOOLSET 再逐条注册）

每次构建 System Prompt：skills::list_enabled_for_prompt_with_config()
Skill 被加载后：        skills::skill_astro_tools_with_config() → 增量开放工具域
运行中注册：            ToolRegistry::register_dynamic(entry, handler)
```

**不变量（最重要的一条）**：

> `StepContext` 冻结当前 Step 的工具、路由与配置快照；**热加载或后续 Skill 激活
> 不能让已经发出的请求获得更宽的执行边界**。新的可见性从**下一次采样**开始生效。

`tool_search` 激活的 Deferred 工具同理：只在后续 Step 生效。

工具身份如何表达（`(namespace, child)` canonical 路由、保留前缀与冲突拒绝）见 §3.4。

**现状锚点**：`runtime/mod.rs::reload_tools_and_mcp`、`runtime/{step_context,tool_router}.rs`、
`agent-tools/src/engine/registry.rs`、`agent-skills/src/installed.rs`、`runtime/system_prompt.rs:177`。

**建议补强**

- **P0｜热加载需要「变更审计」**：当前重载是静默的，rollout 里看不到「本 turn 新增了哪些工具/技能」。
  建议生成 `CapabilityDiff { added, removed, source }` 事件写入 rollout，并在 UI 上给出一次性提示。
  验收：安装一个新 Skill 后，下一 turn 的 rollout 含 `CapabilityDiff.added`，且被发出的调用仍只按旧快照执行。
- **P0｜动态注册需要原子性**：`reload_mcp_from_snapshot` 采用「先卸 toolset 再逐条注册」，
  中途失败会留下**部分可用**状态。建议改为 staging → swap（构建完整条目集，校验通过后一次性替换），
  失败则保留上一版并写失败事件。验收：注入一个注册失败的 MCP 条目，断言注册表仍是旧版本且事件被记录。
- **P1｜热加载时延要可测**：把 reload 纳入 trace timing（gate / MCP / workflow 分段），
  否则「装了却没生效」只能靠猜。验收：`trace insights` 能看到 reload 各段耗时。
- **P2｜能力来源标注**：每个 `ToolEntry` 标注来源（builtin / skill / mcp / workflow / extension），
  让 UI 与审计都能回答「这个工具是谁给的」。

### 3.4 工具命名空间与 canonical 路由（必答项 ★）

**目标**：命名空间是 wire 层的一等公民。工具身份是 `(namespace, child_name)` 结构对，
不是拼接出来的字符串；模型侧、路由侧、分发侧共用同一个 canonical identity，
任何「展平名伪装」都必须失败。

**机制**

```text
ToolName::{ Plain(name) | Namespaced { namespace, name } }
  wire_name() = "namespace.name"
  namespace 为空或等于 DEFAULT_FUNCTION_NAMESPACE("functions") → 降级为 Plain

ToolEntry { name: registered_name, namespace, model_name?, ... }
  ToolEntry::tool_name()：由 registered_name 剥离 "{namespace}__"（其次 "{namespace}_"）
                          得到 child name，再构成 canonical identity

ToolRouter（每个 Step 冻结一份）
  routes       = 本 Step 全部可执行 identity（含已激活的 Deferred）
  model_routes = 允许模型顶层直调的 identity（routes 的真子集）
  canonical_names: HashMap<ToolName, registered_name>   ← 冲突即 fail-closed
```

**五类 wire tool 与命名空间**

| Responses wire type | 命名空间表现 | Astro 侧 |
| --- | --- | --- |
| Function | 无命名空间（默认 `functions`） | `ToolName::Plain` |
| Freeform | 同 Function，参数为自由文本 | 保留原生 `freeform_format` |
| Namespace | `namespace.tools[]`，子工具各自可带 `defer_loading` | `ToolName::Namespaced`；`tool_search` 输出时按 namespace 合并 |
| ToolSearch | 顶层检索工具，自身无命名空间 | `tool_search`，返回可加载的 function / namespace spec |
| WebSearch | Provider 托管检索（部分实现） | 与客户端 Deferred `web_search` 的语义需收敛 |

**当前命名空间清单（以源码常量为准）**

| 来源 | 模型侧命名空间 | 内部形态 |
| --- | --- | --- |
| 内置媒体 | `media`（`image_gen` / `tts` / `video_gen` / `music_gen`） | `registered_name` + `namespace = "media"` |
| 浏览器 | `astro_browser`（`browser_open` → `astro_browser.open`） | 内部仍按 `browser_*` 路由与授权 |
| Workflow | `workflow` | 每 Step 从 WorkflowStore 重建并冻结在 `ToolRouter` |
| Cron | `cron` | 计划任务工具 |
| MCP | `mcp__{sanitized_server}` + 原生子工具名 | 内部限定名 `mcp__{server}__{tool}` 只在 `McpHub` 分发边界使用 |
| 其余内置 / Skill | 无命名空间（默认 `functions`） | `ToolName::Plain`；Skill 经 `astro_tools` 增量开放工具域，**不新造 wire 命名空间** |

**不变量（fail-closed）**

1. 同一 canonical identity 映射到两个不同注册名 → 构建 router 即失败
   （实测文案：`tool identity collision for ...`），不依赖 HashMap 顺序；
2. 模型可见但缺少 `CoreToolRuntime` 的条目 → 构建 router 即失败；
3. 模型顶层直调必须落在 `model_routes` 内：Deferred 与 `CodeModeOnly` 的工具
   不能被展平名或猜测名直接调用；
4. `tool_search_output` 只授权**下一次 Step** 的 Deferred 激活，不修改注册表 exposure；
5. 未注册的 `mcp__*` 一律拒绝——**前缀本身不构成授权**；
6. CodeMode 的 JS 标识符归一化是**有损**的（`read-page` 与 `read_page` 可能同名），
   嵌套路由发生冲突时不得任选一个 runtime；
7. Server 边界即命名空间边界：`mcp__a` 与 `mcp__b` 的同名子工具不得互相遮蔽。

**现状锚点**：`agent-types/src/tool_entry.rs`（`ToolName`、`DEFAULT_FUNCTION_NAMESPACE`）、
`agent-core/src/runtime/tool_router.rs`（`model_routes`、canonical 冲突检测）、
`agent-tools/src/engine/{registry,catalog}.rs`（`schemas_for_step` / `build_tool_router`）、
`agent-mcp/src/names.rs`（`MCP_PREFIX` / `tool_namespace` / `qualify_tool_name`）、
`agent-tools/src/builtin/shell/browser.rs`、`builtin/media/mod.rs`、
`builtin/memory/scheduled.rs`、`engine/workflow.rs`。

**建议补强**

- **P0｜保留命名空间前缀保护**：`mcp__`、`astro_browser`、`workflow`、`media`、`cron`
  应由宿主集中声明为保留前缀，Skill / Extension / MCP server id 不得占用，越界即 fail-closed 并给出诊断。
  验收：注册一个名为 `mcp__x__y` 的 extension 工具被拒绝，且诊断指出冲突的保留前缀。
- **P0｜审计与 UI 同时记录 canonical identity**：只记 `mcp__srv__ns_read_file` 这类限定名会产生歧义；
  审计行需同时包含 wire `(namespace, child)`、注册名与来源。
  验收：从导出的审计行可反查注册名与来源模块。
- **P1｜冲突诊断 typed 化**：canonical 冲突 / 缺 runtime / 未注册 `mcp__*` / 展平名伪装四类
  当前以字符串 `bail!` 呈现，建议收敛为 typed denial，便于 UI 聚合与测试断言。
- **P1｜namespace 级检索预算**：`tool_search` 合并 namespace 后，应为单个 server/namespace
  设子工具上限，避免一个大 MCP server 挤占整次搜索预算（与 §3.5 的字节预算配套）。
- **P2｜命名空间常量收口**：`astro_browser`、`media`、`workflow`、`cron` 目前分散在各模块常量中，
  建议集中声明一份命名空间注册表，供 router、审计与文档共用。

### 3.5 tool_search 与延迟发现（必答项 ★）

**目标**：模型不必背全量工具 schema；按需检索、按需激活，且检索结果本身构成**可审计的授权凭据**。

**当前机制**

```text
tool_search(query, limit=10)
  → BM25 检索 {registered_name, wire_name, toolset, description}
  → 返回原生可加载 spec（function / namespace，defer_loading: true）
  → 同 namespace 结果合并（coalesce）
  → ToolSearchOutput 作为唯一可发现性授权
  → 下一次 Step 才可直调
```

**不变量（fail-closed 清单）**

- 拒绝格式非法的 Provider schema；
- 拒绝服务端执行的 `tool_search_call`；
- `Function` 参数 JSON 非法 → 在审批与 handler 之前转成 `args_parse_error`；
- 元数据被替换后，旧 `ToolRuntime` 立即失效；
- 拒绝重复的 canonical routed identity；拒绝未注册的 `mcp__*`；
- `ToolSearchOutput` 只是**下一步**的 Deferred 授权，不是永久开关。

**现状锚点**：`agent-tools/src/builtin/shell/tool_search.rs`、
`agent-tools/src/engine/registry.rs`（`schemas_for_step` / `build_tool_router`）、
`agent-core/src/runtime/tool_router.rs`、测试 `agent-tools/tests/tool_search_alignment.rs`。

**建议补强**

- **P0｜检索质量对中文与本地生态不友好**：当前 BM25 使用 `Language::English` 分词，
  而 Astro 的工具与 Skill 描述大量为中文；中文 query 命中率会明显劣化。建议：
  文本归一化（中英混排分词 / 名称与描述分权重）、toolset 名与 namespace 别名扩容、
  最近使用与学习信号加权。验收：构造中英文用例集，`tool_search` top-3 命中率达标。
- **P0｜检索结果需要字节预算**：`limit=10` 的语义是「条数」，但 namespace 合并且工具 schema
  可能很大，一次搜索就能吃掉可观的上下文。建议改为**字节/Token 预算优先**：
  按分数贪心填充至预算上限，超限条目标记为「存在但未返回」。验收：超大 schema 工具集下，
  单次 `tool_search` 输出不超过设定预算。
- **P1｜空结果的引导**：无命中时只返回空数组，模型倾向反复重写 query。建议返回
  「最接近的 toolset 名 + 可用类别摘要」作为提示。验收：故意查询不存在能力时，
  输出含引导信息且不再触发重复重试。
- **P1｜激活集合要进快照指纹**：把「本 Step 因哪次 tool_search 获得了哪些工具」并入 `StepContext` 指纹，
  使审计能回答「这个调用凭什么被允许」。验收：rollout 中可回放「搜索 → 激活 → 调用」三段证据链。

### 3.6 上下文管理（必答项 ★）

**目标**：上下文是**预算化资源**；压缩只影响模型视图，**永不丢失原文**。

**机制：三段压缩 + 双视图**

```text
maintain_tool_context()
  1) prune          # 截断超大 tool 结果
  2) 辅模型摘要      # AuxiliaryTask::Compaction
  3) head/tail 兜底  # 仍超限时保留首尾
  + thrashing guard # 同轮连续压缩不生效，防抖

存储分离：
  content            = 原文，永不改写
  compressed_content = Provider 视图（元数据 astro_compressed_output 存 stub）
大结果：≥ DEFAULT_SPILL_THRESHOLD_BYTES → 落盘 sessions/tool_spills/，模型只看到 stub
```

**ContextUsage 三层口径**

```text
provider_reported  >  provider_recomputed  >  local_estimate   （顶层 total）
                 + local segments（system / tools / MCP / memory / conversation）
```

**现状锚点**：`agent-core/src/runtime/context_maintenance.rs`、`compression.rs`、
`runtime/compression_state.rs`、`prompt/{context,context_usage,context_state}.rs`、
`exec/tool_llm_compress.rs`。

**建议补强**

- **P0｜压缩要可解释**：压缩目前对用户是黑箱。建议把压缩事件（触发原因、压缩前后 token、被压缩条目 id）
  写入 rollout，并在 UI 提供「本轮为何被压缩」入口。验收：任意压缩 turn 都能列出触发原因与被压缩范围。
- **P1｜压缩前置预算**：现为事后维护。建议在 Step 前做 estimate + 预留输出预算，
  避免「先超限再补救」；超限时优先压缩工具结果而非最近对话。
- **P1｜原文回取通道**：`content` 与 spills 已保留原文，但缺模型侧回取手段。
  建议把被压缩/落盘的条目暴露为可寻址句柄（thread 内 `history read`），
  让模型在需要时按需取回，而不是要求永不压缩。

### 3.7 记忆系统（必答项 ★）

**目标**：长期记忆是**受治理的写入**，不是模型随手 append 的文本。

**机制**

| 层 | 内容 | 写入方式 |
| --- | --- | --- |
| 工作区快照 | `SOUL.md` / `USER.md` / `MEMORY.md` | 由 MemoryManager 管理，随 PromptContract 注入 |
| 当日上下文 | 日记 / daily | 运行时读取 |
| 召回 | FTS5 召回（turn ≥ `recent_turns=10` 触发） | 只读检索，作为独立 developer 上下文 |
| 待审批写入 | `memory/pending/`（`enqueue` / `list_pending` / `approve` / `reject`） | 模型提出，人批准后落盘 |
| dreaming | `memory/dreaming.json` 驱动 | 离线管道，产出待审批项 |
| 决策日志 | `decision_log` | 记录决策与依据 |
| 权限审计 | `permission_audit` | 记录审批相关事件 |

**现状锚点**：`crates/agent-memory/src/`、`crates/agent-artifacts`（`knowledge.db`，FTS）。

**建议补强**

- **P0｜记忆写入要带来源引用**：当前 pending 项缺「来自哪个 session/turn/item」的强引用，
  批准后无法回溯证据。建议在 `PendingMemoryWrite` 中加入 `session_id/turn_id/item_id` 与内容哈希。
  验收：批准一条记忆后，可一键跳回原始对话证据。
- **P1｜可撤销与版本化**：建议为 `MEMORY.md` 引入带版本的追加式变更记录（谁、何时、依据哪次批准），
  支持撤销单条记忆而不整文件回滚。
- **P1｜dreaming 产出需 diff 预览**：离线管道应产出「拟新增/拟删除」的结构化 diff，
  经审批界面确认后写入，避免整段重写覆盖人工内容。

### 3.8 Subagent 与审批授权（必答项 ★）

**目标**：多 Agent 是**受控能力**，不是权限放大器；审批覆盖主聊天、子 Agent、MCP 与桌面弹窗。

**机制：子 Agent**

- 唯一模型：V2 Agent Threads，模型侧只有六个工具
  `spawn_agent` / `list_agents` / `send_message` / `followup_task` / `wait_agent` / `interrupt_agent`；
- `send_message` 只入队；`followup_task` 入队并触发/恢复 turn；`wait_agent` 等待任意 mailbox/final/steer；
- 状态固定：`PendingInit` / `Running` / `Interrupted` / `Completed` / `Errored` / `Shutdown`；
- Graph / mailbox / status → `sessions/subagents/subagents-v2.db`；真实对话 → `sessions/state.db`；
- **权限继承父任务且只可收窄**；凭证只在内存传递；**不隐式创建 git worktree**。

**机制：审批分级**

| 机制 | 作用 | 锚点 |
| --- | --- | --- |
| HITL gate / registry | 交互式工具挂起并等待解决 | `control/hitl.rs` |
| Smart approval | 辅模型裁决 `approve_once` / `approve_session` / `deny` / `ask`，失败回退 `Ask` | `control/smart_approval.rs` |
| 会话审批缓存 | `ApprovalCacheKey` 复用本会话同类批准，支持 `derive_child_cache` 派生给子会话 | `control/approval_cache.rs` |
| 渐进信任 | 同命令前缀连续批准 → `SmartReview`(5) → `AutoApprove`(10)，**仅 session 内有效** | `control/trust_model.rs` |
| 网络审批 | `ApprovalScope::{Once, Session, Persistent}`；未 resolve 即 drop → 全部 waiter 视为拒绝 | `control/network_approval.rs` |
| MCP elicitation | Broker 承接 MCP 侧询问，统一进 HITL 通道 | `agent-mcp`、`runtime/mod.rs` |

**不变量**：未知行为默认拒绝；未解决的审批所有权被 drop 时 fail-closed；
审批必须绑定到**同一次执行尝试**，不得跨 attempt 复用 lease。

**建议补强**

- **P0｜审批去重**：同一 actionable request 不得同时弹主聊天与桌面宠物（历史缺陷形态）。
  建议在 `HitlRegistry` 层引入稳定 request key（session + tool + 参数哈希 + attempt），
  配合 `InteractionSnapshot` 的 epoch/revision 与 Tauri 侧 `uiRevision` 防陈旧覆盖。
  验收：同一请求在任一界面处理，另一界面自动消解且不重复询问。
- **P0｜子 Agent 授权矩阵测试**：补齐「父授 session / 子必须再问 / 子无权提级 / 派生缓存收窄」四类用例。
  验收：`cargo test -p agent --control` 覆盖矩阵，且新增收窄路径有失败用例。
- **P1｜审批可解释**：smart approval 的裁决理由应写入 rollout（风险等级、依据），
  而不是只留最终 decision，否则审计无法解释「为什么这次被自动放行」。

### 3.9 生成式 UI（A2UI）（必答项 ★）

**目标**：模型可以生成**声明式界面**（表单、向导、结果卡），但 UI **不能**成为业务状态的唯一载体。

**机制**

- Catalog：`astro://a2ui/catalog/v2`，允许组件以 `crates/agent-a2ui/src/catalog.rs::ALLOWED_COMPONENTS` 为准（当前 21 个，含 `ClarifyWizard`）；
- 所有操作经 `validate.rs` 做 catalog 校验后再渲染；
- 模板驱动常用形态（表单、澄清向导、结果面板）；
- **业务状态由 `TurnItem`/`ExtensionItem` 承载并持久化**，A2UI 负责动态布局与表单；
  例如 `image_gen` 显式映射为 `TurnItem::ImageGeneration`，从而复用 started/completed/持久化/投影链路。

**现状锚点**：`crates/agent-a2ui/`、`agent-protocol` 的 `TurnItem`/`ExtensionItem`、
Desktop 端 A2UI 渲染与测试（`apps/desktop/src/a2ui/`）。

**建议补强**

- **P0｜提交幂等**：表单/向导提交必须带客户端幂等键，防止恢复重放导致重复副作用。
  验收：同一提交在重放与重连场景下只产生一次业务效果。
- **P1｜再水合测试**：断言「重启后 A2UI surface 能由持久化业务状态重建」，
  而不是依赖内存中的 surface 状态。验收：重启后表单状态与结果卡与重启前一致。
- **P2｜A2UI 与审批打通**：把审批卡统一走 A2UI 声明式组件，减少一次性 UI 代码，
  同时保持 `uiRevision` 防陈旧语义。

### 3.10 安全护栏

**目标**：安全不是 handler 末尾的一个 `if`，而是沿链的多层裁决。

| 层 | 决策 |
| --- | --- |
| 暴露 | tool gate、Skill 增量授权、`ToolExposure` 五级 + `ToolMode` |
| 快照 | `StepContext` 拒绝本 Step 未暴露的模型调用 |
| 意图 | interaction mode + 参数 schema 校验 |
| 授权 | smart approval、MCP approval、HITL、session approval cache、渐进信任 |
| 隔离 | `SandboxPolicy`、workspace roots、网络策略 |
| 扩展 | `PreToolUse` 可 block/modify；结果可 transform |
| 审计 | tool call、approval、sandbox denial、usage、rollout |

**网络**：默认放开（子进程 `allow network*`，进程内 HTTP 无白名单），
但**始终保留 SSRF 防护**（仅 http/https，拦截本机/私网/云 metadata，重定向逐跳校验）。
启用 managed proxy 后，lease 只归属单个 tool attempt，只放行精确绑定的 loopback 端口；
结构化网络拒绝**不得**触发文件系统提权；502/DNS/dial 错误不是 policy denial。

**建议补强**

- P0：把「未知行为默认拒绝」写成可执行测试矩阵（未知工具名 / 未知 namespace / 未注册 `mcp__*` /
  未知 catalog 组件 / 未知 hook 事件名），当前分散在各 crate，建议集中为一份 fail-closed 套件。
- P1：沙箱拒绝的结构化原因（`SandboxErr::Denied` 分类）应进入统一审计导出，
  便于回答「这次没执行是因为策略还是因为环境」。

### 3.11 状态溯源与可观测（可恢复 / 可审计）（必答项 ★）

**目标**：任何一次执行都能在事后被完整重放与举证，且崩溃不产生歧义。

**机制**

- **事实源**：append-only rollout（本地文件，逐 Thread）；
- **投影**：`state.db`（WAL + FTS5，schema v24）用于查询、FTS 召回与 UI 历史；
- **live**：Server 每 Thread listener → gRPC/Tauri；恢复用 snapshot + live boundary；
- **归因**：`agent-usage` 的 `usage.db` 按 Agent / session / tool 记录 token、成本、延迟，
  含 per-agent `stats.json`、trace insights、eval JSONL 导出；
- **数据库归属**（以 `agent-home/src/workspace/paths.rs` 为准）：

| 库 | 路径 | 职责 |
| --- | --- | --- |
| `state.db` | `sessions/state.db` | ResponseItem、会话、FTS5、线程检查点与附件（v24） |
| `subagents-v2.db` | `sessions/subagents/subagents-v2.db` | Agent Graph、mailbox、状态事件 |
| `usage.db` | `usage/usage.db` | 用量、成本、trace insights |
| `artifacts.db` / `knowledge.db` | `artifacts/` | 文件空间索引 / 知识内容（FTS） |
| `cron_v1.db` | `automation/cron/cron_v1.db` | 定时任务运行记录 |
| `workflow.db` | `automation/workflows/workflow.db` | 工作流运行 |

**建议补强**

- **P0｜统一审计导出**：审批、沙箱、Hook、工具、usage 分散在多处。
  建议提供单一审计导出（JSONL，一行一事件：`turn_id/tool_call_id/attempt/decision/scope/actor/policy`）
  并做**序号连续性 + 哈希链**校验，使审计可自证未被篡改或截断。
  验收：导出文件可被离线校验脚本验证连续性；人为删除一行会被检出。
- **P0｜崩溃注入测试**：在「rollout append 成功 / SQLite 投影未完成」窗口 kill 进程，
  断言恢复后历史完整、无重复副作用、UI 与 live 去重正确。
  验收：新增 crash-injection 集成测试进入 CI。
- **P1｜工具级成本归因**：usage 已按 agent/session 归因，建议补 tool 维度
  （同一 turn 内哪个工具贡献了多少 token / 时间），使「贵在哪」可回答。
- **P1｜恢复演练指标**：记录恢复耗时、重建 item 数、去重命中数，纳入 trace insights。

### 3.12 MCP 与多环境接入

**目标**：把异构能力（终端、浏览器、MCP、媒体、Cron、Workflow）统一封装成受控工具。

| 环境 | 接入形态 | 关键约束 |
| --- | --- | --- |
| 终端 / `exec_command` | 受沙箱与审批管理 | managed proxy 时后台模式在 spawn 前拒绝 |
| 浏览器 | 任务绑定隔离会话 | 允许 `astro_browser.<child>` 前缀，内部保留 `browser_*` 路由 |
| MCP | 每 Agent 进程级连接池 `McpHub`，工具名 `mcp__{server}__{tool}` | 延迟发现 + 调用时审批；远端默认按网络策略可连 |
| 媒体 | 图像 / TTS / 视频 / 音乐 Provider | 复用 Harness 的持久化与观测 |
| Cron | 30s ticker，`current_thread` runtime | `AgentLoop`/`SessionStore` 非 Send，必须 `spawn_blocking` 包装 |
| Workflow | 每 Step 从 WorkflowStore 重建 `workflow` namespace | Deferred 工作流只能由可信 `tool_search_output` 激活 |

**建议补强**：MCP event-stream manager 的 opener 与 Desktop 订阅 RPC 仍未接线（已知未闭环项），
建议作为 M1 的收口目标，否则 MCP 侧推送能力无法端到端验收。

---

## 4. 关键设计决策（ADR 摘要）

| 决策 | 理由 | 代价 |
| --- | --- | --- |
| 事实源是 rollout 而非 SQLite | 崩溃后必须能重建，查询库易被写坏或落后 | 需要投影一致性与去重逻辑 |
| 暴露 / 可执行 / 授权三者正交 | 「模型看得到」不等于「可以执行」，更不等于「已授权」 | 契约更复杂，需要 fail-closed 清单 |
| Step 快照冻结工具面 | 热加载不得改变已发出调用的语义 | 新能力延迟一个 Step 生效 |
| 压缩保留 `content`，只改模型视图 | 审计与回取依赖原文 | 存储与实现成本上升 |
| A2UI 不承载业务状态 | UI 是投影，业务状态必须可持久化与恢复 | 需要显式 TurnItem 映射 |
| 子 Agent 权限只可收窄 | 多 Agent 不能成为提权跳板 | 子任务需重新授权，交互略重 |
| 网络默认放开 + 保留 SSRF | 本地工具链需要真实网络能力 | 依赖审批与沙箱承担边界责任 |

---

## 5. 建议补强清单（按优先级汇总）

### P0（缺了就无法验收「可热加载 / 可恢复 / 可审计」）

| # | 建议 | 交付物 | 验收 |
| --- | --- | --- | --- |
| 1 | 热加载变更审计 `CapabilityDiff` → rollout + UI 提示 | 事件类型 + 投影 + UI | 装新 Skill 后下一 turn 可见 diff，旧快照调用边界不变 |
| 2 | 动态注册原子化（staging → swap，失败保留旧版） | registry 事务化 | 注入失败条目后注册表仍为旧版且事件落盘 |
| 3 | `tool_search` 中文/混排检索质量 + toolset 加权 | 归一化 + 权重 | 中英用例集 top-3 命中率达标 |
| 4 | `tool_search` 结果字节预算 | 预算优先填充 | 超大 schema 下单次输出不超预算 |
| 5 | 压缩可解释（原因/前后 token/范围）写 rollout + UI 入口 | 事件 + UI | 任意压缩 turn 可解释 |
| 6 | 记忆写入带来源引用（session/turn/item + 哈希） | pending schema | 批准后可跳回原始证据 |
| 7 | 审批去重（主聊天 / 桌宠 / 子 Agent 同一 request） | HitlRegistry key + revision | 一处处理，其他界面自动消解 |
| 8 | 子 Agent 授权矩阵测试（含收窄与提级拒绝） | 测试 | 四类矩阵用例通过 |
| 9 | A2UI 提交幂等键 | 协议 + 客户端 | 重放只产生一次业务效果 |
| 10 | 统一审计导出 + 连续性/哈希链校验 | 导出器 + 校验脚本 | 删行可被检出 |
| 11 | 崩溃注入测试（rollout↔投影窗口） | 集成测试 | 恢复完整、无重复副作用 |
| 12 | 保留命名空间前缀保护（`mcp__` / `astro_browser` / `workflow` / `media` / `cron`） | 保留前缀表 + 构建期校验 | 越界注册被拒绝并给出诊断 |
| 13 | 审计与 UI 同时记录 canonical `(namespace, child)`、注册名与来源 | 审计字段 + 导出 | 审计行可反查注册名与来源模块 |

### P1（显著提升可用性与可信度）

- 批量工具并行的显式批次语义与并发上限；批次 id 入 rollout；
- 取消原因枚举，替代统一「中断」；
- `BudgetPressure` 事件 + wall-clock / 工具次数预算；
- Step 前上下文预算预估（压缩前置）；
- 被压缩/落盘条目的可寻址回取句柄；
- 记忆版本化与撤销；dreaming 产出 diff 预览；
- smart approval 裁决理由入 rollout；
- usage 增加 tool 维度归因；
- 恢复演练指标（耗时 / 重建数 / 去重命中）进 trace insights；
- MCP event-stream opener 与 Desktop 订阅接线；
- 命名空间冲突 typed denial；namespace 级 `tool_search` 子工具预算；命名空间常量集中声明。

### P2（体系化与体验）

- `ToolEntry` 来源标注（builtin / skill / mcp / workflow / extension）；
- 审批卡统一走 A2UI 声明式组件；
- fail-closed 集中测试套件；
- 沙箱拒绝原因进入统一审计；
- Provider-hosted WebSearch 与客户端 Deferred `web_search` 的语义收敛；
- 历史 Code Mode 残留清理。

---

## 6. 度量与验收指标

| 指标 | 定义 | 目标 | 验证方式 |
| --- | --- | --- | --- |
| 热加载生效时延 | 安装/启用到下一 turn 生效的耗时 | 不重启即可生效，且 diff 可见 | 端到端用例 + rollout 断言 |
| 工具快照越界率 | 已发出调用因热加载获得更宽边界的次数 | **0** | `tool_search_alignment` 类测试 |
| Deferred 越权率 | 未激活工具被直调的放行次数 | **0**（fail-closed） | fail-closed 套件 |
| 命名空间越界率 | 保留前缀被 Skill / Extension / MCP server id 占用的次数 | **0** | 注册校验测试 |
| 上下文压缩可解释率 | 能给出原因与范围的压缩比例 | 100% | rollout 事件断言 |
| 崩溃恢复正确率 | kill 后历史重建与去重正确的比例 | 100% | crash-injection 测试 |
| 重复副作用 | 恢复/重放产生的重复执行次数 | **0** | 幂等键 + 副作用计数断言 |
| 审计完整率 | 具备 tool_call_id/attempt/decision/scope 的调用比例 | 100% | 导出校验脚本 |
| Schema token 占用 | 默认注入的工具 schema token | 仅 Direct 工具；Deferred 经检索 | usage/trace segments |
| 审批等待时长 | 从挂起到解决的中位时长 | 持续观测并下降 | trace insights |
| 归属覆盖率 | 可归因到 agent/session/tool 的成本比例 | 100% | usage.db 查询 |

---

## 7. 里程碑（0 → 1 路线）

| 阶段 | 目标 | 交付物 | 出口条件 |
| --- | --- | --- | --- |
| M0 基线冻结 | 把「已实现 / 部分实现 / 目标设计」三态写进文档与测试 | 指标埋点 + 基线测试 | `cargo check` / 定向测试全绿，指标可采集 |
| M1 热加载与边界 | 热加载可审计、注册原子化、命名空间保留前缀、fail-closed 集中 | `CapabilityDiff`、staging→swap、保留前缀表、fail-closed 套件 | P0#1、#2、#8、#12 通过；越权率为 0 |
| M2 检索与预算 | `tool_search` 质量与预算；上下文压缩前置与可解释 | 归一化检索、字节预算、压缩事件 | P0#3、#4、#5 通过 |
| M3 恢复与审计 | 崩溃注入、统一审计导出、工具级与命名空间级归因 | crash 测试、审计导出器、usage tool 维度 | P0#10、#11、#13 通过；审计完整率 100% |
| M4 记忆 / UI / 多 Agent 闭环 | 记忆来源与版本、A2UI 幂等与再水合、审批去重 | pending schema 升级、幂等键、去重键 | P0#6、#7、#9 通过 |

每阶段收尾统一执行：`cargo fmt --all`、`cargo clippy --all-targets`、定向 `cargo test`、
`cd apps/desktop && npx tsc --noEmit`，并按仓库约定提交（不 push）。

---

## 8. 风险与对策

| 风险 | 表现 | 对策 |
| --- | --- | --- |
| 热加载与快照语义耦合 | 新能力在错误时机生效，产生越权 | 快照冻结作为硬不变量 + 越界率指标为 0 |
| 检索质量不足 | 模型找不到工具，退化为反复重试 | P0#3/#4 + 空结果引导 |
| 压缩丢证据 | 事后无法复盘 | 原文永存 + 来源引用 + 可寻址回取 |
| 审批疲劳 | 用户习惯性批准，安全形同虚设 | 渐进信任 + 会话缓存 + 去重 + 理由可解释 |
| 事件源与投影不一致 | UI 与 rollout 分叉 | snapshot + live boundary + 去重 + 崩溃注入测试 |
| 子 Agent 成为提权跳板 | 越权访问父级资源 | 只可收窄 + 授权矩阵测试 |
| 本地数据损坏 | 迁移后出现空平行库 | 保持 `require_current_layout` 拒绝初始化旧布局 |
| 文档与实现漂移 | 声称已实现但无源码入口 | 见 §10 待修项；「已实现」必须能定位源码与验证入口 |

---

## 9. 现状锚点映射

| 能力 | 主要事实源 |
| --- | --- |
| Thread / submission | `crates/agent-core/src/runtime/{astro_thread,session_io,submission_loop}.rs` |
| Session / Turn / Step | `runtime/{session,turn_context,step_context,turn_lifecycle}.rs` |
| 模型工具循环 | `streaming/{multi_turn,tools_exec,fallback}.rs` |
| Prompt / 上下文 | `prompt/{contract,context,context_source,context_usage}.rs`、`runtime/system_prompt.rs` |
| 压缩 | `runtime/{context_maintenance,compression_state}.rs`、`compression.rs`、`exec/tool_llm_compress.rs` |
| 工具注册与路由 | `crates/agent-tools/src/engine/{registry,dispatch,catalog}.rs`、`runtime/tool_router.rs` |
| 命名空间 / canonical 路由 | `crates/agent-types/src/tool_entry.rs`（`ToolName`）、`runtime/tool_router.rs`（`model_routes`、冲突检测）、`crates/agent-mcp/src/names.rs`、`agent-tools/src/builtin/{shell/browser,media/mod,memory/scheduled}.rs`、`engine/workflow.rs` |
| `tool_search` | `crates/agent-tools/src/builtin/shell/tool_search.rs`、`tests/tool_search_alignment.rs` |
| Skills | `crates/agent-skills/src/{installed,registry,install}.rs` |
| 记忆 | `crates/agent-memory/src/{lib,pending,dreaming,decision_log,permission_audit}.rs` |
| 审批 / HITL | `crates/agent-core/src/control/{hitl,smart_approval,approval_cache,trust_model,network_approval}.rs` |
| 沙箱 / 网络 | `crates/agent-sandbox/src/`、`crates/agent-network-proxy/src/` |
| 子 Agent | `crates/agent-subagents/src/` |
| 生成式 UI | `crates/agent-a2ui/src/{catalog,validate,templates}.rs`、`apps/desktop/src/a2ui/` |
| 事件源 / 恢复 | `crates/agent-rollout/src/{recorder,reconstruction,policy}.rs` |
| 查询投影 | `crates/agent-session/src/store/`、`agent-server/src/thread_listener.rs` |
| 用量 / 成本 | `crates/agent-usage/src/` |
| MCP | `crates/agent-mcp/src/` |
| Cron / Workflow | `crates/agent-{cron,workflow}/src/` |
| 路径与布局 | `crates/agent-home/src/{layout.rs,workspace/paths.rs}` |

---

## 10. 顺手发现的文档漂移（建议修正）

1. `docs/05-质量审查阶段/05-Agent-Harness文档一致性审查.md` §2 表格中
   「SQLite 职责库统一位于 `{base}/data/`」与当前实现相反：`data` 属于
   `agent-home/src/layout.rs::RETIRED_LAYOUT_PATHS`（旧布局迁移标记），
   权威路径为 `sessions/state.db`、`sessions/subagents/subagents-v2.db`、`usage/usage.db`、
   `artifacts/*.db`、`automation/*/*.db`（见 `workspace/paths.rs` 测试
   `canonical_database_paths_belong_to_their_domains`）。
2. `AGENTS.md` 记 A2UI 为「22 种组件」，而
   `crates/agent-a2ui/src/catalog.rs::ALLOWED_COMPONENTS` 当前为 21 项（含 `ClarifyWizard`）。
3. `docs/03-系统设计阶段/01-架构设计/11-Agent-Harness总体架构.md` §11 描述子 Agent 状态库为
   `{base}/subagents.db`，与 `workspace/paths.rs` 的 `sessions/subagents/subagents-v2.db` 不一致。

以上三项均为**表述漂移**，不涉及行为变更；建议随下一次 Harness 文档更新一并修正。
