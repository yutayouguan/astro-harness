# Agent Harness 项目经历（面试官视角）

> 用途：简历条目 + 面试口述稿 + 追问题库，全部基于 Astro 仓库可核验的事实。
>
> 说明：本文按「你主导 Astro Harness 层设计与实现」撰写。若实际是团队分工，把「主导/负责」
> 替换为真实角色即可，其余内容不变。
>
> 原则：**只写能当场取证的**。规模数字见 §8 复核命令，未实测的收益类指标一律留空并标注，
> 面试时被追问「这个数怎么来的」比数字本身更重要。

---

## 0. 面试官在看什么（先对齐评分表）

| 面试官真正在评的 | 这份材料在哪交付 |
| --- | --- |
| 你解决的问题有没有真实难度 | §1 条目 2/3/5（执行边界、事件溯源、上下文预算） |
| 你是「调 API 的人」还是「建系统的人」 | §3 架构层级 + §4 难点拆解 |
| 你做没做取舍，还是只会堆功能 | §4 每个难点都写了「代价」；§5 追问 3/15 |
| 你知不知道自己系统的短板 | §7 已知边界 + 明确列出**不能说的三句话** |
| 你是否可被追问三层还不崩 | §5 十五问 + §6 举证清单 |

一句话策略：**用结构性事实（层级、契约、不变量）证明深度，用可运行的举证证明真实性，
用主动暴露的短板证明你不是背稿。**

---

## 1. 简历版（30 秒读完）

**项目**：Astro Agent —— 本地优先的多模态 AI Agent 桌面工作站
**技术栈**：Rust（tokio / tonic gRPC）+ SQLite（WAL / FTS5）+ Tauri 2 + React / Vite + MCP
**角色**：Agent Harness 层架构设计与实现

**规模**：29 个 workspace package（28 Rust crate + 1 Tauri 应用）· 24.1 万行 Rust（604 个文件）
· 16.2 万行 TS/TSX · 2,583 个 Rust 测试声明 · 37 条 gRPC RPC · ~350 个 Tauri command · 424 篇设计文档

**核心条目**（建议放 4–5 条，按目标岗位取舍）：

1. **设计并落地「Agent = Model + Harness」的执行外壳**：把 LLM 意图转成可预算、可审批、可恢复、
   可审计的执行。实现 `submit → reason → act → observe` 单活跃任务闭环，划分
   Thread / Session / Task / Turn / Step / Attempt 六级运行层级，明确「热加载不得改变已发出调用语义」
   等硬不变量。
2. **以 append-only 事件流为唯一事实源，SQLite 只做查询投影**：崩溃或重启后按
   「rollout snapshot + live boundary」重建执行历史，历史与实时按稳定 item/turn identity 去重，
   避免「猜最后一条消息」重放副作用；投影层用 WAL + FTS5 支撑检索与 UI。
3. **把「模型可见性 / 可执行性 / 授权」三者解耦**：设计 `ToolExposure` 五级
   （Direct / Deferred / Hidden / ModelOnly 两态）+ `ToolMode` 三档，每个 Step 冻结工具与路由快照；
   运行中热加载工具、MCP、Skill 只影响**下一次采样**，越界调用一律 fail-closed。
4. **实现原生 `tool_search` 延迟工具发现与工具命名空间契约**：模型不必背全量 schema，
   按需 BM25 检索并激活 Deferred 工具与 MCP 元数据；工具身份以 `(namespace, child_name)`
   canonical 形式路由，canonical 冲突、未注册 `mcp__*`、展平名伪装全部在路由构建期拒绝。
5. **预算化上下文与受治理的长期记忆**：prune → 辅模型摘要 → head/tail 三段压缩 + 同轮防抖，
   原文永久保留、模型只读压缩视图，超大工具结果自动落盘只暴露 stub；记忆以
   SOUL/USER/MEMORY 快照 + FTS5 召回 + 待审批写入队列治理，模型不能直接改写长期记忆。
6. （可选，命中安全岗/基础架构岗时放）**尝试级安全模型**：审批、沙箱、网络策略、Hook 决策
   绑定到同一次工具执行尝试，managed proxy 的租约只放行精确绑定的 loopback 端口；
   HITL + 辅模型裁决 + 会话审批缓存 + 渐进信任构成分级授权，未知行为默认拒绝。

---

## 2. 2 分钟口述版（STAR）

**S（背景）**：本地优先的 AI 桌面工作站要让模型真的动文件、跑命令、连 MCP、开浏览器。裸模型
只会「说」，一旦开始「做」，就必须有人回答六个问题：多步循环归谁、上下文预算归谁、工具边界怎么定、
危险动作谁批、崩溃后怎么重建、事后怎么举证。

**T（目标）**：在模型之外建一层 Harness，把模型意图变成**可预算、可审批、可恢复、可审计**的执行，
而不是给模型挂一堆工具函数。

**A（我做了什么）**

- 定义 **Agent = Model + Harness** 并落到代码：外部只有 `Op` 入口、内部只有一条
  submission loop、对外只暴露 `EventMsg`/`TurnItem`，任何绕过工具路由直接执行 handler 的路径
  都算架构违规。
- 把执行分成六级（Thread/Session/Task/Turn/Step/Attempt），**每个 Step 冻结一份工具与配置快照**，
  这样热加载、Skill 激活、MCP 变更都只能影响下一步，绝不扩宽已经发出的请求的边界。
- 把工具的「可见性 / 可执行性 / 授权」拆成三个正交维度，延迟工具由原生 `tool_search` 发现，
  检索结果本身作为**下一步的授权凭据**，其余情况一律拒绝。
- 用 append-only 事件流当唯一事实源，SQLite 只是查询投影，恢复走 snapshot + live boundary 去重。
- 上下文做成预算：三段压缩 + 原文/模型视图分离 + 大结果落盘 + 同轮防抖；记忆做成
  「模型提议 → 人批准 → 才落盘」的治理流程。

**R（结果）**：系统跑出了 29 个 package、24 万行 Rust、2,583 个测试声明、37 条 gRPC RPC 的规模；
Harness 的七类职责（执行循环 / 上下文记忆 / 工具中介 / 权限安全 / 错误恢复 / 可观测审计 / 运行环境）
都有对应的代码与测试锚点，而不是文档承诺。

---

## 3. 架构层级（面试官问「画一下」时用）

```text
入口    Desktop(React/Tauri) | gRPC 客户端 | Cron | Subagent runner
协议    agent-protocol::{Op, EventMsg, TurnItem, ResponseItem}
会话    AstroThread → Session → submission_loop → SessionTask/RegularTask
回合    TurnContext → prepare_turn → PromptContract（稳定指令 + 动态上下文）
步骤    StepContext（工具/路由/配置快照）→ Responses 流式请求
执行    ToolAccumulator → ToolRouter → 审批 → 沙箱/网络 → Hooks → handler/MCP
事实    rollout(append-only) → SQLite 投影 → live listener(gRPC/Tauri)
```

六级生命周期与硬不变量：

| 层级 | 生命周期 | 不变量 |
| --- | --- | --- |
| Thread | 会话级长生命周期 | 单 submission loop，I/O 只绑定一次 |
| Session | 会话共享状态 | 单活跃 turn，录取与事件分发串行 |
| Task | 一次可取消工作 | 安装新任务前必须中止旧任务 |
| Turn | 一条用户意图 | `tool_rounds` 归零，输入准入可开关 |
| Step | 一次采样 + 工具执行 | 工具/路由/配置快照不被热加载突变 |
| Attempt | 一次真实执行尝试 | 审批与网络租约只属于本次尝试 |

---

## 4. 技术难点与解决方案（面试主战场）

### 难点 1：热加载不能在运行中改变「已发出调用」的语义

- **问题**：用户装了个 Skill、连了个 MCP，工具集变了。如果注册表是全局可变的，模型在这一步
  发出的调用可能被新的路由解释成另一个工具——这是隐蔽的越权与不可复现 bug。
- **方案**：注册发生变更，但**每个 Step 冻结自己的快照**。工具身份用
  `ToolName::{Plain, Namespaced{namespace, name}}` 表达并通过 `ToolRouter` 冻结；
  同一个 canonical identity 映射到两个注册名时直接拒绝构建，而不是依赖 HashMap 顺序任选一个。
  模型可见但缺少执行运行时的条目也会拒绝构建。
- **代价**：新能力延迟一个 Step 生效；用户"刚装的技能怎么没反应"需要产品层解释。
- **结果**：这个不变量有直接回归测试，不靠口头承诺——
  `tool_search_alignment.rs::deferred_tool_remains_deferred_after_registry_reregistration`
  （重新注册后延迟工具仍是延迟状态），以及 `tool_router.rs` 的三条构建期拒绝用例
  （见难点 4）。

### 难点 2：多步循环里的上下文爆炸

- **问题**：一个任务几十轮，每轮工具结果可能几十 KB，直接回灌必然超窗；粗暴截断又会丢证据。
- **方案**：三段式维护——prune（截断超大工具结果）→ 辅模型摘要 → head/tail 兜底，
  并加同轮防抖（连续压缩不叠加）。存储上分离**原文**与**模型视图**：原文
  `content` 永不改写，模型只读压缩视图；超过阈值的工具结果落盘，只向模型暴露 stub。
- **代价**：存储与实现复杂度上升；需要额外的引用与清理策略。
- **结果**：可复盘的原文 + 可控的上下文占用；用 Provider 上报/recomputed/本地估算三层口径
  解释上下文构成，避免"总额对了但不知道为什么"。

### 难点 3：崩溃恢复不能重复副作用

- **问题**：进程被 kill 在「事件已写」和「投影已写」之间，重启后怎么知道执行到哪？
- **方案**：确定唯一事实源是 append-only rollout，SQLite 只是查询投影。恢复用
  `rollout snapshot + live boundary`，并且**恢复不等于重执行**；历史与 live 通过稳定
  item/turn identity 去重。计量上区分「本轮累计」和「最近一次采样」，fork 不继承累计值，
  避免跨分支重复计费。
- **代价**：需要维护投影一致性与去重逻辑；写入路径必须先落事实源再更新投影。
- **结果**：恢复语义可解释、可审计；副作用重放被结构性地排除，而不是靠"小心"。

### 难点 4：工具身份与命名空间

- **问题**：工具来自内置、MCP、Workflow、Skill、Extension，名称会撞；MCP 里不同 server
  可能有同名子工具；某些前缀被上游 Provider 保留。
- **方案**：命名空间是 wire 层一等公民。工具身份是 `(namespace, child)` 结构对，
  `wire_name` 才拼成 `namespace.name`；`mcp__{server}` 作为 server 边界命名空间，
  内部限定名只在分发边界使用；浏览器工具用 `astro_browser.<child>` 绕开上游保留名，
  内部仍按 `browser_*` 路由。CodeMode 的 JS 标识符归一化是**有损**的（`read-page` 与
  `read_page` 可能同名），因此冲突时不允许任选一个运行时。
- **代价**：契约变复杂，需要集中维护保留前缀与冲突诊断。
- **结果**：跨来源同名不再互相遮蔽；冲突在构建期就失败，而不是运行时随机挑一个。
  对应回归测试：`build_tool_router_rejects_duplicate_canonical_identity`、
  `build_tool_router_rejects_visible_metadata_without_runtime`、
  `code_mode_rejects_lossy_identifier_collisions`、
  `build_tool_call_rejects_invalid_json_before_dispatch`。

### 难点 5：审批不能与执行脱钩

- **问题**：如果审批只挂工具回调，沙箱、网络策略、Hook 各自为政，就会出现"批了 A 却放了 B"。
- **方案**：把审批、沙箱、网络、Hook 决策绑定到**同一次 tool attempt**；受管网络代理的租约
  只归属单次尝试，沙箱只放行它精确绑定的 loopback 端口，不扩散到其他工具或 Provider。
  授权分级：交互式 HITL、辅模型裁决（失败回退到「问人」）、会话级审批缓存（可派生给子会话）、
  渐进信任（同命令前缀连续批准后升级，仅会话内有效）。所有权被 drop 而未解决时一律视为拒绝。
- **代价**：交互变重；需要防止「审批疲劳」导致用户无脑点同意。
- **结果**：默认拒绝 + 分级授权的边界体系，而不是一个全局开关。

### 难点 6：生成式 UI 不能成为状态载体

- **问题**：模型生成的表单/卡片很直观，但如果业务状态只存在 UI 内存里，重启即丢、也无法审计。
- **方案**：声明式 UI 走**固定 catalog 校验**，业务状态由持久化的 `TurnItem` 承载，
  UI 只是投影；例如图片生成显式映射为对应的 TurnItem，从而复用既有的开始/完成/持久化/投影链路。
- **代价**：需要为每类业务补显式映射，不能"随便画"。
- **结果**：重启后界面可由状态重建，A2UI 只负责动态布局与表单。

### 难点 7：把非 Send 的运行时安全地塞进异步世界

- **问题**：核心运行时持有非 Send 的 SQLite 连接，不能在多线程 runtime 里随便跑。
- **方案**：定时任务固定在单线程 runtime 上运行，对外用 `spawn_blocking` 包装成 Send future；
  SQLite 的初始化收敛为进程生命周期内一次的幂等操作，避免每连接重复初始化。
- **代价**：并发模型受限，需要明确线程归属。
- **结果**：Cron、后台任务与前台事件流共用同一套执行链，不再维护第二套 headless 循环。

---

## 5. 面试官追问 15 问（附答题要点）

1. **为什么不直接用 LangChain / LlamaIndex 这类框架？**
   回答方向：它们的抽象中心是"链/索引"，我要解决的是执行边界、审批、崩溃恢复与审计；
   本地优先还要求数据不出机、能力可换、离线可自证。框架可以借，但 Harness 的六问必须自己回答。

2. **`submit → reason → act → observe` 里最容易被做错的是哪一步？**
   Observe。很多人把工具结果直接塞回消息列表；正确做法是先记录成可恢复的结构化项，
   再构造 Provider 视图，并且保证调用与输出成对、顺序可回放。

3. **怎么保证热加载不越权？**
   三个动作：Step 快照冻结、canonical 身份校验（冲突即拒绝构建）、默认拒绝未知命名空间/未注册前缀。
   一句话总结：**新能力只能从下一次采样开始生效**。

4. **`tool_search` 为什么用 BM25 而不是向量检索？**
   工具检索的查询短、候选集小、术语高度专属（工具名+描述），BM25 无需外部依赖、可离线、结果可解释。
   承认短板：当前分词对中文描述不友好、结果上限按条数而非按字节预算，这是我下一步要改的。
   （**主动说短板 = 加分项**）

5. **工具结果压缩后，模型要原文怎么办？**
   原文永远保留在存储层，模型只读压缩视图；压缩只是"视图"而不是"覆盖"。
   理想形态是给被压缩条目一个可寻址句柄，让模型按需回取——目前这条链路还不完整，属于已知缺口。

6. **崩溃恢复怎么保证不重复执行副作用？**
   rollout 是唯一事实源，恢复是"重建视图"而不是"重放动作"；投影与实时流按稳定 identity 去重；
   副作用类操作走两阶段准入（先持久化去重标记再执行）。

7. **SQLite 为什么不能直接当事变源？**
   它是查询库：schema 会演进、可能被迁移/清理、写入路径可能失败。事实源要求 append-only
   且可重放。我的做法是事实源与投影分离，投影挂了可以重建。

8. **审批为什么会和沙箱耦合？**
   因为它们裁决的是同一件事：这次尝试能不能碰这个资源。绑定到 attempt 之后，
   "批了但没执行/执行了但没记"的不一致面就消失了。

9. **多 Agent 怎么防止提权？**
   子 Agent 继承父任务权限且**只能收窄**；不隐式创建 worktree；凭证只在内存传递；
   模型侧只暴露六个线程级工具（spawn/list/send/followup/wait/interrupt），
   真正的"读时间线、递归关闭"是控制面动作而不是模型能力。

10. **批量工具并发执行时，持久化顺序怎么保证？**
    执行可以并发，落盘必须有序且带批次归属。当前批次语义还不是一等公民——
    这是我列进演进清单的改进项（显式批次 id + 并发上限）。

11. **token 与成本口径为什么不混用？**
    一条用户输入可能触发多次采样。"本轮累计"用于计费，"最近一次采样"用于校准上下文环，
    两者混用就会出现"上下文环显示 90% 但单次请求其实没满"的错误。

12. **生成式 UI 怎么保证不是玩具？**
    固定 catalog + 校验 + 业务状态持久化在 `TurnItem`；UI 只是投影，可重建、可审计。

13. **你怎么测试这套系统？**
    分层：单元测试覆盖契约与校验；集成测试覆盖跨 crate 链路（工具路由、事件恢复、Provider）；
    前端有组件与交互测试；CI 三个 workflow（build-tauri / quality / release-tauri）。
    不足：缺少崩溃注入与端到端恢复演练，这是明确的下一个补齐项。

14. **如果重做，你会改什么？**
    ① 热加载原子化（先卸后装的中间态）；② `tool_search` 的中文检索与字节预算；
    ③ 保留命名空间前缀保护；④ 崩溃注入测试进 CI；⑤ 统一审计导出 + 连续性校验。
    （这五条都是自证型回答，说明你在做架构而不是补 bug）

15. **这个项目最大的缺陷是什么？**
    复杂度与可发现性：六个运行层级 + 三个正交维度很强，但新加入的人很难一眼看出
    "一次调用到底经过了哪些关卡"。我的应对是把不变量写成文档 + 测试断言，
    并让每条能力都能在代码里定位到事实源。

---

## 6. 现场举证清单（面试官怀疑时直接给）

| 想证明 | 给什么 |
| --- | --- |
| 规模不是吹的 | `cargo metadata --no-deps --format-version 1 \| jq '.packages \| length'` → 29 |
| 测试不是摆设 | `rg -o "^\s*#\[(tokio::)?test\]" -g '*.rs' crates apps \| wc -l` → 2583 |
| 架构有层级 | `crates/agent-core/src/runtime/` 下 thread/session/turn/step/attempt 对应文件 |
| 事件溯源真实存在 | `crates/agent-rollout/src/{recorder,reconstruction,policy}.rs` |
| 工具边界可验证 | `crates/agent-core/src/runtime/tool_router.rs` 的 canonical 冲突拒绝 |
| 关键不变量有回归测试 | `cargo test -p agent build_tool_router_rejects` / `-p tools deferred_tool_remains_deferred` |
| 命名空间契约 | `crates/agent-types/src/tool_entry.rs`（`ToolName`）+ `agent-mcp/src/names.rs` |
| 上下文策略 | `agent-core/src/runtime/{context_maintenance,compression_state}.rs` |
| 审批模型 | `agent-core/src/control/{hitl,smart_approval,approval_cache,trust_model}.rs` |
| 生成式 UI | `crates/agent-a2ui/src/{catalog,validate}.rs` + Desktop 渲染层 |
| 文档密度 | `docs/` 下 424 篇 Markdown（含架构/详细设计/质量审查分阶段） |

可现场演示的三件事（能跑起来最有说服力）：

1. 装一个新 Skill → 下一轮对话直接具备该能力（无需重启）；
2. 观察 `tool_search` 前后注入模型的工具 schema 变化（延迟发现的效果）；
3. 运行中 kill 进程 → 重启后线程历史完整、无重复执行。

> 提示：第 3 条目前最好是"演示 + 说明测试缺口"，不要宣称已有完整回归；
> 面试官一旦追问细节，超出证据的宣称就是减分。

**四个可以直接念出名字的回归测试**（证明关键不变量不是口头承诺）：

```text
crates/agent-tools/tests/tool_search_alignment.rs
  ├─ deferred_tool_remains_deferred_after_registry_reregistration   # 热加载不扩宽边界
  └─ registry_emits_native_responses_tool_shapes                    # 原生 wire 形态

crates/agent-core/src/runtime/tool_router.rs（单元测试）
  ├─ build_tool_router_rejects_duplicate_canonical_identity         # 工具身份冲突即拒绝
  ├─ build_tool_router_rejects_visible_metadata_without_runtime     # 可见但不可执行即拒绝
  ├─ build_tool_call_rejects_invalid_json_before_dispatch           # 参数非法先于审批被拦
  └─ code_mode_rejects_lossy_identifier_collisions                  # JS 标识符歧义不任选
```

以上 5 个用例已于 2026-09-16 在当前工作区实跑通过（`cargo test -p agent <用例名>`、
`cargo test -p tools <用例名>`）；`agent` crate 的单元测试总数为 434。
面试时可以直接说「这些不变量有回归测试，我可以现场跑给你看」。

---

## 7. 已知边界与「不能说的三句话」

**当前明确的未闭环项**（文档中已标注，面试时主动说边界更可信）：

- 远端扩展市场依赖自有服务与信任根，当前只激活本地扩展；
- 原生语音助手仍走 WebView/WebRTC，签名三平台产物未落地；
- MCP 事件流的订阅端尚未接线；
- CONNECT 代理看不到 TLS 内部，头部注入目前只做解析与脱敏；
- HTTP CONNECT 之外（plain HTTP / SOCKS）的受管代理尚未实现。

**不能说的三句话**（说了会被追问到崩）：

1. 「和 Codex/Claude Code 完全对齐」——对齐是逐项语义对齐，且有明确差异清单；
2. 「支持 XX 个内置工具 + XX 个技能」——数量随环境变化，除非当场能跑出来；
3. 「性能提升了 X%」——没有基准就不要给百分比，改用结构性结论（如"默认注入面从全量收敛到 Direct 工具 + 按需检索"）。

---

## 8. 需要你补的真实数据（我无法替你测）

| 指标 | 怎么测 | 建议表述 |
| --- | --- | --- |
| 上下文压缩收益 | 选取长任务，对比压缩前后送入模型的 token | 「长任务上下文占用下降 __%」 |
| 延迟发现收益 | 统计默认注入 schema 的 token vs 全量注入 | 「默认注入面收敛至 __%」 |
| 热加载生效时延 | 装 Skill → 下一轮生效的耗时 | 「__ ms 内生效，无需重启」 |
| 崩溃恢复耗时 | kill 后重启到历史可用的时长 | 「__ s 内完成重建」 |
| 测试规模与通过情况 | `cargo test --workspace` 统计通过数 | 「__ 个测试用例通过」 |
| 迭代节奏 | `git log --oneline \| wc -l` | 3265 次提交（2026-07-13 → 2026-09-16） |

---

## 9. 数据来源与复核命令

本文所有数字均在 `2026-09-16` 从当前工作区实测得到：

```bash
# 29 个 workspace package（28 crate + 1 Tauri 应用）
cargo metadata --no-deps --format-version 1 | jq '.packages | length'

# 24.1 万行 Rust / 604 个文件（不含 target）
find crates apps/desktop/src-tauri -name '*.rs' -not -path '*/target/*' | xargs wc -l | tail -1
find crates apps/desktop/src-tauri -name '*.rs' -not -path '*/target/*' | wc -l

# 2583 个 Rust 测试声明；37 个集成测试文件
rg -o "^\s*#\[(tokio::)?test\]" -g '*.rs' crates apps | wc -l
find crates -path '*/tests/*.rs' | wc -l

# 16.2 万行 TS/TSX；37 条 gRPC RPC；~350 个 Tauri command；424 篇文档
find apps/desktop/src -name '*.ts' -o -name '*.tsx' | xargs wc -l | tail -1
rg -o "^\s*rpc [A-Za-z]+" crates/agent-proto -g '*.proto' | wc -l
rg -o "#\[tauri::command\]" apps/desktop/src-tauri/src | wc -l
find docs -name '*.md' | wc -l

# 3265 次提交，2026-07-13 → 2026-09-16
git log --oneline | wc -l

# 关键不变量的回归测试（2026-09-16 实跑通过）
cargo test -p agent build_tool_router_rejects_duplicate_canonical_identity
cargo test -p agent build_tool_router_rejects_visible_metadata_without_runtime
cargo test -p agent build_tool_call_rejects_invalid_json_before_dispatch
cargo test -p agent code_mode_rejects_lossy_identifier_collisions
cargo test -p tools deferred_tool_remains_deferred_after_registry_reregistration
```

相关配套材料：

- [Agent Harness 层项目建议书](2026-09-16-agent-harness-proposal.md)（能力盘点与 P0/P1/P2 演进清单）
- [Agent Harness 总体架构](../03-系统设计阶段/01-架构设计/11-Agent-Harness总体架构.md)（当前实现基线）
- [Agent Harness 文档一致性审查](../05-质量审查阶段/05-Agent-Harness文档一致性审查.md)（已统一的不变量与未闭环项）
