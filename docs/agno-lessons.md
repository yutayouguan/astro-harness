# Agno 借鉴分析：对 Astro 的可复用点

本文汇总对 [Agno](https://github.com/agno-agi/agno)（`libs/agno/agno`）的架构调研，对照 Astro 现状，梳理**可直接复用的设计**、**Astro 已强于 Agno 无需照搬的部分**，以及**建议落地顺序**。

本轮对照以本地源码为准：`/Users/iswm/CodeRope/agno-agi/agno/libs/agno/agno`。

调研分两块：
1. 多模态 / 多 Provider / 统一工具 / Agent 生命周期 / hooks / tracing / session / skills
2. 上下文管理 / 记忆 / 学习 / 知识库 / DB / MCP

---

## 一、多模态与多 Provider

### Agno 做法
- 统一媒体对象：`Image` / `Audio` / `Video` / `File`，可作输入与工具结果。
- `Model` 抽象基类 + 各厂商**原生 SDK**实现（不把所有厂商都塑成 OpenAI 形状）。
- 媒体在整个链路（输入、消息、工具结果、输出）都是一等公民。

### Astro 现状
- Provider 已按 `google/` `openai/` 模块拆分，Google 走 Interactions，OpenAI 走 Responses/compat。
- 媒体处理分散在 `providers/src/*/media_*`、`tools/src/builtins/media/`。

### 可借鉴
1. **`MediaAsset` 一等抽象**：统一 `Image/Audio/Video/File`，贯穿输入 → 消息 → 工具结果 → 输出。
2. **结构化 `ToolResult`（可带媒体）**：工具不仅返回文本，也能返回图片/音频等，直接进上下文。
3. 保持「原生 SDK per vendor」而非强行 OpenAI 化——Astro 的 Google Interactions 迁移已印证这个方向正确。

### 已落地（本轮）
- `common::media::{MediaAsset, MediaKind, MediaRef}`：对齐 Agno Image/Audio/Video/File 的统一引用
- `Message.media` + `ToolResult.media`：结构化附件；发给 LLM 仍用文本摘要
- 生成工具（`image_gen` / `tts` / `video_gen` / `music_gen`）附加 `astro_media_v1:` sidecar
- 流事件 `MultiTurnStreamItem::ToolResult.media` → proto `ToolCallEvent.media` → Tauri/UI
- 前端 `MsgActivity` 优先用结构化 `activity.media`，回落 `parseGeneratedMedia`（含 sidecar）
- **未做**：改写 `ChatContentPart` 支持 audio/video 进 LLM；用户附图 `media_json` 列持久化（hydrate 仅从 tool sidecar 提取；见 Review）

## 二、统一工具调用与 Agent 生命周期

### Agno 做法
- 富 `Function` 元数据：`confirmation`（需确认）、`hooks`（前后钩子）、`stop_after`（执行后停止）、结果可含媒体。
- 显式 `Run` / `Session` / 事件模型：run 有清晰状态机与事件流。

### Astro 现状
- ToolRegistry + MCP 桥接（`mcp__server__tool`），有 hooks（`InjectContext` 等）。
- 有 session、streaming provider，但缺显式「Run 状态机 / requirements」一等模型。

### 可借鉴
1. **显式 Run 状态机 / requirements**：把「等待确认 / 等待工具 / 等待用户」建模为状态，而非散落判断。
2. **工具元数据扩展**：`confirmation` / `stop_after` 与现有审批流对齐。
3. **Session 消息重建规则**：处理悬挂 tool 消息（有 tool_call 无对应结果）的重建/清理规则。

### 已落地（本轮）
- `ToolEntry.needs_confirmation` / `stop_after_tool_call`：对齐 Agno Function 元数据；HITL 工具默认 `needs_confirmation`
- `ToolRegistry::any_needs_confirmation` / `any_stop_after`：`multi_turn` 串行门控与「执行后结束 run」
- `agent::streaming::run_state::{RunPhase, RunRequirements, RunState}`：派生 `RunFinished.outcome_type`（含 HITL）
- `prompt::sanitize::sanitize_tool_pairs`：发送 Provider / hydrate session 前清理悬挂 tool_calls 与孤儿 tool 消息
- `is_interactive_tool` 已 deprecated；串行门控以 `needs_confirmation` 为准（§三）
- **未做**：工具级 hooks 仍走现有 hooks crate（刻意保留）

---

## 三、hooks / tracing / session / skills

### Agno
- hooks：函数级前后钩子。
- tracing：run 内详细 trace，可用于调试与构建 eval 数据集。
- skills：渐进式披露 + 按需暴露工具。

### Astro
- hooks crate 已有；usage/trace 旁路；Skills 安装 + 摘要机制完整。

### 可借鉴
- Skills 的「渐进式披露」Astro 已具备，可继续强化「按需加载工具」而非全量注入。
- Trace 数据可导出为 eval 数据集（复用现有 usage + session，不必新造监控栈）。

### 已落地（本轮）
- `multi_turn` 串行门控改用 `ToolEntry.needs_confirmation`；`is_interactive_tool` 标 deprecated
- `usage::eval_export::export_session_eval_jsonl`：session trace → JSONL eval 行（复用 usage.db）
- Skill frontmatter 可选 `astro_tools`；`skills` 工具加载后 `ToolRegistry::activate_skill_toolsets` **additive 放宽**禁用 toolset
- Hooks：本轮不新增 per-tool hooks，继续用现有 `pre/post_tool_call` 总线

---

## 四、上下文管理（Context）

### Agno
- `ContextProvider`，按模式注入（`default` / `agent` / `tools`）。
- Agent 可主动查改上下文（agentic context）：`search_context` / 更新工具。
- session history / memory / knowledge 分层组合进 prompt。

### Astro 现状
- `StaticContext` / `DynamicContext` + FTS 召回拼进 system prompt。
- hooks `InjectContext` / `queue_inject_context` 临时注入；有 Context Explorer 与用量估算。
- 超长靠 `SessionStore::compact_and_split` 拆新会话。

关键路径：
- `agent/src/prompt/context.rs`
- `agent/src/prompt/context_usage.rs`
- `session/src/store/messages.rs`(`compact_and_split`)

### 可借鉴
1. **统一 `ContextSource` trait + 预算**：session / memory / FTS /（未来）KB / inject 各自 `contribute(budget)`，替代在拼 prompt 处堆叠逻辑。
2. **Agentic context 工具**：按需 `search_context` / `pin_context`，而不是每轮塞满 Dynamic。
3. 不必照搬 Agno 的 Python Context 全家桶；Astro 分层 prompt 已够用，缺的是「预算 + 协议」。

### 已落地（本轮）
- `agent::prompt::context_source::{ContextSource, ContextBudget, assemble_system_layers}`
- `build_system_prompt` 按 static → skills → guidance → timestamp → dynamic 共享字符预算
- `AgentConfig.context_budget_chars`（默认 200_000）
- hooks `InjectContext` / `queue_inject_context` 仍走消息侧 `[astro:hook-context]`（不进 system，避免双重注入）
- **未做**：`search_context` / `pin_context` agentic 工具

---

## 五、运行中压缩（最值得复用）

### Agno `CompressionManager`
- 阈值：未压缩 tool 结果条数，或 token 上限。
- 对 `role=tool` 消息做 LLM 摘要，写入 `compressed_content`，**保留原文、仅改发送视图**。
- 明确保留：数字、日期、实体、ID、URL；去掉套话与排版噪音。
- 计入 usage（compression model）。

### Astro 现状（压缩）
- 已有会话级压实（`compact_and_split`）。
- 本轮已补 run 内 tool 结果压缩（见下「已落地」）；辅助模型压缩路由可后续替换启发式。

### 可复用设计（建议直接落地）
```text
messages 中 tool 结果超阈值
  → 异步/同步压缩
  → 发送模型用 compressed_content，DB/UI 仍保留原文
  → 计入 usage（compression model）
```
与现有压实互补：压实管**会话生命周期**，压缩管**单次 run 的 context 预算**。

### 已落地（本轮）
- `common::Message.compressed_content` + session schema v14 `messages.compressed_content`
- `agent::compression::ToolCompressionManager`：未压缩 tool 条数 ≥ 3 时触发
- `AgentLoop::compress_tool_results_if_needed` 接入 `multi_turn`
- `to_provider_messages` 对 tool 角色优先发送压缩视图；FTS/UI 仍用原文
- 当前压缩实现为确定性启发式（头/尾保留）；后续可接 auxiliary compaction 模型

---

## 六、记忆管理（Memory）

### Agno
- `MemoryManager` + 优化策略（summarize / prune）。
- 偏「库表事实」+ 检索进 context。

### Astro（已强）
- 文件精炼记忆：`MEMORY.md` / `USER.md` + daily notes，字符上限 + Frozen Snapshot 注入。
- `write_approval` pending 队列；入梦（daily → MEMORY）；background review。
- UI/文档完整（Memory 面板、slash `/memory`）。

关键路径：`memory/src/lib.rs`、`memory/src/dreaming/mod.rs`、`memory/src/pending.rs`、`memory/src/review.rs`

### 可借鉴（轻量）
- 把 MEMORY/USER/daily 抽象成 **MemoryStore 协议**（list / search / propose_write / apply）。
- Agno 的「策略对象」（summarize / prune）挂到入梦/review，而非替换 Markdown 产品形态。
- **不必**为对齐 Agno 改成纯 DB 记忆；Astro 的审批流是差异化优势。

### 已落地（本轮）
- `memory::protocol::{MemoryOps, FileMemoryOps, MemoryWriteIntent}`：list/search/propose_write/apply
- `propose_or_apply` 尊重 `write_approval`（Propose → pending；否则直接 apply）
- 产品形态（Markdown / 入梦 / 审批）不变

---

## 七、学习管理（Learning）— 概念层最值得复用

### Agno `LearningMachine`
多 Store 协同：

| Store | 内容 |
|-------|------|
| UserProfile | 结构化用户档案字段 |
| UserMemory | 非结构化观察 |
| EntityMemory | 公司 / 人 / 项目等实体知识 |
| SessionContext | 会话上下文 |
| LearnedKnowledge | 学到的知识 |
| DecisionLog | 决策日志 |

- **LearningMode**：`Always` / `Agentic` / `Propose`（+ HITL）。
- 每类 Store 可开关 `enable_agent_tools`。

### Astro 映射
| Agno Store | Astro 近似 | 缺口 |
|------------|------------|------|
| UserProfile | `USER.md` | 无字段 schema |
| UserMemory | `MEMORY.md` + daily | 无 entity 图 |
| SessionContext | session + compact | — |
| LearnedKnowledge | skills / 无 | 无「学到即入库」 |
| DecisionLog | usage/trace 旁路 | 无一等决策日志 |
| EntityMemory | — | **缺** |

### 可借鉴优先级
1. **Propose 模式** ≈ 现有 `write_approval`，可推广到「学到的实体 / 决策」。
2. **DecisionLog**：工具失败、用户纠错、关键选择 → 结构化记一笔，供 review/入梦。
3. **EntityMemory（可选）**：`(type, name, facts[], sources[])`，先 FTS、后向量。
4. Always 全自动写库对桌面助手风险高，默认 **Propose / Agentic** 更贴 Astro。

### 已落地（本轮）
- `memory::decision_log::{DecisionEntry, DecisionKind, append_decision, list_recent}`
- JSONL：`~/.astro/learning/decisions.jsonl`
- 挂点：pending `reject` → `MemoryRejected`；工具执行失败 → `ToolFailure`
- **未做**：EntityMemory / Always 模式 / LearnedKnowledge 入库

---

## 八、知识库（Knowledge / RAG）

### Agno
- Content DB：内容可见、可删、可改 metadata、有处理状态。
- 向量检索 + chunk + embedder + rerank + agentic filter。
- Agentic RAG：运行时按需搜，而非塞进 system prompt。

### Astro 现状
- 会话 FTS5、Skills、artifacts 索引；Provider 上 `supports_embedding` 仅**能力位预留**。
- **基本缺失**：向量 DB、embedding 流水线、chunking、RAG 索引/检索。

关键路径：`session/src/store/search.rs`(FTS)、`artifacts/src/db.rs`、`providers/src/api/trait_.rs`(`supports_embedding`)

### 可借鉴（若做 KB）
1. **Content DB 先行**：文档登记 → 状态 → 删除连带清理；比一上来接向量更重要。
2. **检索先 FTS，再 embedding**（与 session 同一 SQLite 风格）。
3. 检索结果走 **citation**，对齐 Agno 的 source attribution。
4. Skills 当作「打包好的 knowledge 包」，KB 管「用户文档」。

### 已落地（本轮）
- `artifacts::KnowledgeDb`（`sessions/knowledge.db`）：`contents` 表 + FTS5 `contents_fts`
- API：`register` / `list` / `search` / `delete`（删登记+FTS）
- **未做**：embedding / rerank / agentic filter

---

## 九、DB

### Agno
- 可插拔多后端（sqlite / postgres / …），统一存 session / memory / trace / eval。

### Astro
- 本地多 SQLite：session（sessions/messages/FTS5）、usage、cron、orchestration、artifacts。

关键路径：`session/src/store/schema.rs`、`session/src/store/mod.rs`、`usage/src/db.rs`

### 可借鉴
- 统一 **`Db` trait**：`sessions` / `memories` / `knowledge_contents` / `traces` 接口形状对齐，实现仍 rusqlite。
- **不要**为对齐 Agno 急上 Postgres；桌面场景价值低。
- Trace/eval 数据集可从现有 usage + session 导出。

### 已落地（本轮）
- `usage::sqlite_store::{open_wal, SqliteStore, ExampleSqliteStore}`：WAL 打开 + path/migrate 协议
- 示范 migrate 空库；不合并多库、不上 Postgres
- Knowledge/Usage 等现有库可逐步 `impl SqliteStore`

---

## 十、MCP

### Agno
- `MCPTools` / `MultiMCPTools`，挂到 `Agent.tools`。

### Astro（已更完整）
- 独立 `mcp` crate：多服务器 Hub、按 Agent 配置持久化、工具发现缓存、`mcp__server__tool` 限定名桥接。
- 传输：stdio + streamable HTTP。

关键路径：`mcp/src/hub.rs`、`mcp/src/config.rs`、`mcp/src/names.rs`

### 可借鉴（边角）
- 生命周期：connect / refresh / disconnect 与 Agent run 绑定更清晰。
- 工具 schema 变更时的失效策略。
- **不必**重写；Astro 这边可视为「已复用完成」。

---

## 十一、Team（多 Agent）

### Agno
Agno 的 Team 是一等运行时，而不是简单「Agent 调 Agent」。核心类型集中在本地源码 `team/`：

- `team/mode.py`：`TeamMode.coordinate` / `route` / `broadcast` / `tasks`
- `team/team.py`：`Team` 门面，包含 leader model、members、session_state、history 共享选项
- `team/_default_tools.py`：`delegate_task_to_member(s)` 委托工具
- `team/task.py`：`tasks` 模式的 `Task` / `TaskList`
- `run/team.py`、`session/team.py`：`TeamRunOutput` / `TeamRunEvent` / `TeamSession`

四种模式的语义：

| Agno `TeamMode` | 语义 |
|-----------------|------|
| `coordinate` | 默认 supervisor：Leader 选择成员、分派任务、综合结果 |
| `route` | 路由到一个专家，成员输出直接作为 Team 输出 |
| `broadcast` | 同一任务发给所有成员，并行收集结果 |
| `tasks` | Leader 维护共享任务列表，循环执行直到目标完成 |

### Astro 现状
Astro 已有可复用积木，但还没有 Agno 式一等 Team 抽象：

- `delegate/` + `agent/src/exec/delegate.rs`：回合内并行瞬时子 Agent，子 Agent 独立 session，摘要回父
- `orchestration/` + `agent/src/exec/orchestration.rs`：异步串行流水线，SQLite 状态机，可续跑
- `tools/src/builtins/agents/multi_agent.rs`：多 Agent 工具入口，薄封装编排
- `home` 的持久 Agent：可作为 Team 成员池
- Insights 协作图：已有 handoff 可观测基础

缺口是「Team 配置 + 模式策略 + 团队状态」：

| Team 概念 | Astro 现状 | 缺口 |
|-----------|------------|------|
| Team 实体 | 一次性工具参数 / 编排 DB 行 | 无持久 `TeamDefinition` |
| Leader | 当前聊天主 Agent 隐式承担 | 无 Team 级 leader 策略 |
| Members | 临时角色或已有 `agent_id` | 无成员表与路由约束 |
| Modes | 并行 delegate / 串行 orchestration | 无 `coordinate` / `route` / `broadcast` 门面 |
| 共享状态 | 显式 context / step output 接力 | 无 Team 级 session_state / blackboard |

### 可借鉴
1. **先做 Team 门面，不重写编排栈**：用现有 delegate/orchestration/create_agent 作为执行底座。
2. **模式先落前三个**：
   - `coordinate`：Leader 可委派 Team 成员，随后继续合成。
   - `route`：委派一个成员后直接返回成员结果。
   - `broadcast`：同一任务并行发给所有成员，再汇总。
3. **TeamDefinition 持久化**：`~/.astro/teams/{id}.json`，成员可引用已有 `agent_id` 或临时角色。
4. **共享状态先轻量**：先做 `team_run_state` JSON merge；`tasks` 共享任务图、嵌套 Team、完整 `TeamRunEvent` 后续再上。

### 已落地（本轮）
- `orchestration::team`：`TeamDefinition` / `TeamMode` / `~/.astro/teams/{id}.json`
- 工具：`team_list` / `team_create` / `team_run`（toolset=`multi_agent`）
- `team_run` 模式映射：
  - `coordinate`：对全部成员发起 delegate，Leader 继续合成
  - `route`：单成员直出（多成员时需 `member_id`），`respond_directly=true`
  - `broadcast`：对全部成员并行 delegate
  - `tasks`：可持久化，但本轮 `team_run` 拒绝执行
- 成员执行仍走现有 delegate runtime；成员 `agent_id` 会写入定义，但本轮执行层先按角色/说明作为临时子 Agent 跑

---

## 建议落地顺序

```text
P0  Mid-run tool 结果压缩（Agno CompressionManager）—— 与 compact_and_split 正交，改动面可控
P0.5 TeamDefinition + coordinate/route/broadcast 门面（复用 delegate/orchestration）
P1  ContextSource trait + budget（协议化现有 Static/Dynamic/FTS）
P2  DecisionLog + Propose 写入（挂审批，扩展入梦/review）
P3  Knowledge Content DB + FTS（可选再 embedding）
P4  EntityMemory（有真实「记公司/项目」需求再上）
—   MediaAsset + 结构化 ToolResult（§一已落地竖切；ChatContentPart 多模态入模 / 用户附图持久化后续）
—   MCP / 多后端 DB：观望或小改即可
```

---

## 一句话总结

- **直接复用思路**：工具结果压缩阈值 + `compressed_content`、TeamMode 门面、LearningMode（尤其 Propose）、Content DB 生命周期、Context 可插拔预算、MediaAsset/结构化 ToolResult。
- **已强于 Agno、少抄**：Markdown 记忆 + 审批 + 入梦、本地 Session FTS、MCP Hub。
- **慎抄**：Always 自动学习、一上来全向量 RAG、远程多后端 DB —— 与 Astro 桌面产品形态不匹配。

> 成熟度速览：Memory / Session FTS 厚；MCP client 中厚；Context 压实与注入 中；Learning 薄→无；RAG/embeddings 薄→无（仅预留）。
