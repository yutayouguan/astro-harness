# Agno：Session / Memory / State 与写入模式

> 源码基准：`/Users/iswm/CodeRope/agno-agi/agno/libs/agno/agno`  
> 相关目录：`session/` · `memory/` · `compression/` · `agent/_run.py` · `agent/_messages.py` · `agent/_managers.py` · `agent/_default_tools.py` · `db/schemas/`

本文讲清三件事：

1. Agno 如何把 **Session / State / Memory** 拆成正交层  
2. **分层开关**如何独立控制「读进 context」与「写回存储」  
3. **Always / Agentic / Propose** 三种写入模式的机制与对 Astro 的映射  

---

## 1. 三层模型（职责边界）

```text
┌─────────────────────────────────────────────────────────────┐
│  Memory（跨会话 · 跨 run · 按 user_id）                      │
│  UserMemory 行：精炼事实句 + topics                          │
│  写入：MemoryManager（专用 LLM + DB 工具）                    │
└─────────────────────────────────────────────────────────────┘
                              ▲ 可选注入 system
┌─────────────────────────────────────────────────────────────┐
│  Session（同一对话线程 · 按 session_id）                      │
│  AgentSession = runs[] + session_data + summary + metadata   │
│  消息挂在 RunOutput.messages，不单独 flat log                │
└─────────────────────────────────────────────────────────────┘
                              ▲ session_data["session_state"]
┌─────────────────────────────────────────────────────────────┐
│  State（会话内可变 KV · scratchpad）                         │
│  Dict[str, Any]：工具/prompt 共享，跨 run 持久，不跨 user    │
└─────────────────────────────────────────────────────────────┘
```

| 层 | 作用域 | 存什么 | 持久化键 |
|----|--------|--------|----------|
| **Session** | 一次对话线程 | runs、消息轨迹、summary、媒体元数据 | `session_id` |
| **State** | 会话内可变字典 | 购物清单、进度、flags… | `session_data["session_state"]` |
| **Memory** | 跨会话用户事实 | 精炼观察句 | `user_id`（可选 `agent_id`/`team_id`） |

**Compression**（`compression/manager.py`）不属上述三层：它只压缩 **当前 run 发给模型的 tool 结果视图**（`compressed_content`），原文仍保留。管的是 context 预算，不是长期记忆。

---

## 2. Session 实现要点

### 2.1 数据结构

```python
# session/agent.py
@dataclass
class AgentSession:
    session_id: str
    agent_id / team_id / user_id / workflow_id: Optional[str]
    session_data: Optional[Dict]   # session_name, session_state, images, metrics…
    metadata: Optional[Dict]
    agent_data: Optional[Dict]
    runs: Optional[List[RunOutput | TeamRunOutput]]
    summary: Optional[SessionSummary]
    created_at / updated_at: Optional[int]
```

- **以 Run 为单元**：`upsert_run(run)` 按 `run_id` 覆盖或追加。  
- **消息重建**：`get_messages(last_n_runs, limit, skip_roles, skip_statuses, skip_history_messages)`  
  - 默认跳过 `paused` / `cancelled` / `error` / `regenerated`  
  - 跳过已标记 `from_history` 的消息（避免「历史套历史」）  
  - 去掉无对应 assistant tool_calls 的悬挂 tool 结果  
- **会话摘要**：`SessionSummaryManager` 在 run 结束后用 LLM 生成 `summary` + `topics`，写入 `session.summary`。

### 2.2 Run 生命周期中的 Session I/O

```text
read_or_create_session(session_id, user_id)
  → load_session_state / update_metadata
  → get_run_messages（拼 system / history / user）
  → model + tools
  → upsert_run + db.upsert_session
  → 可选：session summary / user memory（后台）
```

关键文件：`agent/_storage.py`（`read_or_create_session` / `load_session_state`）、`agent/_run.py`、`agent/_session.py`（`save_session`）。

### 2.3 History → Context

开关：`add_history_to_context`（默认 `False`）。

```text
session.get_messages(last_n_runs=num_history_runs, limit=num_history_messages)
  → deepcopy，每条打 from_history=True
  → 可选 filter_tool_calls(max_tool_calls_from_history)
  → 插入 run_messages（当前 user message 之前）
```

无 `db` 时开 history 只会打 warning，不会真的注入。

---

## 3. State 实现要点

### 3.1 存储位置

State **不是独立表**，而是：

```text
AgentSession.session_data["session_state"] : Dict[str, Any]
```

Agent 默认值：`agent.session_state`；run 时可传入覆盖/合并。

### 3.2 合并规则（`load_session_state`）

```text
初始 session_state = agent 默认 ∪ run 参数
若 DB 有 session_state 且 overwrite_db_session_state=False：
    merged = db_state.copy()
    merge_dictionaries(merged, current)   # current 覆盖同名键
    → 实际优先级：run 参数 > DB > agent 默认
若 overwrite_db_session_state=True：
    以本次为准，覆盖 DB
写回 session.session_data["session_state"]
```

运行时还会注入临时键（`current_user_id` / `current_session_id` / `current_run_id` 等），**upsert 前剥掉**，避免污染持久状态。

### 3.3 读 / 写两条路径

| 能力 | 开关 | 行为 |
|------|------|------|
| 读进 prompt | `add_session_state_to_context` | system 追加 `<session_state>…</session_state>` |
| Agent 自写 | `enable_agentic_state` | 注入工具 `update_session_state(session_state_updates: dict)`，改 `run_context.session_state` 并落盘 |

工具入口见 `agent/_default_tools.py` → `make_update_session_state_entrypoint`。

一句话：**State = 会话级 scratchpad**，给工具与 prompt 共享，跨 run 持久，不跨 user。

---

## 4. Memory 实现要点

### 4.1 数据模型

```python
# db/schemas/memory.py
@dataclass
class UserMemory:
    memory: str
    memory_id: Optional[str]
    topics: Optional[List[str]]
    user_id: Optional[str]
    input: Optional[str]          # 触发写入时的用户原文
    agent_id / team_id: Optional[str]
    created_at / updated_at / feedback: …
```

与 Session **分表**：跨 `session_id`，按 `user_id` 聚合。

### 4.2 MemoryManager = 带 DB 工具的专用 Agent

`memory/manager.py` 核心流程：

```text
create_user_memories(messages, user_id)
  → 读出该 user 已有 memories（缩成 {memory_id, memory}）
  → create_or_update_memories：
       system = 捕获指令 + 现有记忆列表
       tools  = add_memory / update_memory / delete_memory / clear_memory（可开关）
       → memory model.response(messages, tools)
       → 工具调用直接 db.upsert_user_memory / delete_user_memory
```

DB 工具实现（节选语义）：

| 工具 | 作用 |
|------|------|
| `add_memory(memory, topics?)` | 新 UUID，upsert |
| `update_memory(memory_id, memory, topics?)` | 覆盖同 id |
| `delete_memory(memory_id)` | 删单条 |
| `clear_memory()` | 清空该 user |

另有 `search_user_memories`（语义检索）、`optimize_memories`（`SummarizeStrategy` 等策略对象）。

### 4.3 读进 Context

开关：`add_memories_to_context`。

```text
system +=
  <memories_from_previous_interactions>
    - memory1
    - memory2
  </memories_from_previous_interactions>
  + 「当前对话优先于旧记忆」提示
  +（若 enable_agentic_memory）update_user_memory 使用说明
```

见 `agent/_messages.py`。

---

## 5. 分层开关（核心可借鉴点）

Agno 把 **「是否读」** 与 **「如何写」** 拆成独立开关，互不耦合。

### 5.1 总表

| 层 | 读进 Context | 写回存储 | 谁决定写 |
|----|--------------|----------|----------|
| **History** | `add_history_to_context` | 每 run 自然 `upsert_run` | 框架 |
| **Session Summary** | `add_session_summary_to_context` | `enable_session_summaries` | 框架（run 结束） |
| **State** | `add_session_state_to_context` | `enable_agentic_state` 或 API `update_session_state` | Agent / 调用方 |
| **Memory** | `add_memories_to_context` | `update_memory_on_run` **或** `enable_agentic_memory` | 框架 Always / Agent Agentic |

配套旋钮：

- History：`num_history_runs` / `num_history_messages` / `max_tool_calls_from_history`  
- State：`overwrite_db_session_state` / `cache_session`  
- Memory：`memory_manager`（模型、捕获指令、`add/update/delete/clear_memories` 权限）  
- Compression：`compress_tool_results` / `compress_tool_results_limit` / `compress_token_limit`  

### 5.2 为何要「分层」而不是一个总开关

1. **成本可控**：只开 history、不开 memory 抽取 → 零额外 LLM；只开 memory 读、不开写 → 静态档案。  
2. **安全边界清晰**：桌面助手可「读 Always、写 Propose」；服务端自动化可「写 Always」。  
3. **调试友好**：关掉某一层即可排查「是历史污染还是记忆串话」。  
4. **产品形态可组合**：聊天机器人常开 history+memory；工作流常开 state、少开 history。

### 5.3 一轮消息的组装顺序

```text
system =
  instructions
  + <memories_…>              # Memory 读
  + <session_state>           # State 读
  + session summary / knowledge / …
messages =
  [history tagged from_history]  # Session history 读
  + current user message
（发送前可选 Compression 改写 tool 视图）
```

读路径全部是 **拼 prompt**；写路径发生在 run **之后或工具调用之中**（见下一节）。

---

## 6. 写入模式：Always / Agentic / Propose

Agno 在 Memory（及 LearningMachine）上明确区分三种模式。State 主要是 Agentic + API；工具层另有 HITL（confirmation / approval）可叠加成 Propose。

### 6.1 Always（自动写）

**开关**：`update_memory_on_run=True`（旧名 `enable_user_memories`）。

**时机**：run 结束（或后台任务），见 `agent/_managers.py`：

```text
if memory_manager and update_memory_on_run and not enable_agentic_memory:
    background: create_user_memories(user_message | extra_messages, user_id)
```

注意：`enable_agentic_memory=True` 时 **不会** 再走 Always 后台任务，避免双重写入。

**特点**：

- 主 Agent 无感知；另起 memory model 调用  
- 延迟可后台化（`start_memory_task` / `start_memory_future`）  
- 适合「用户明确授权长期记忆」的服务端 Agent  
- **风险**：误记、记隐私、无法拦截 → 桌面助手默认不推荐裸 Always  

### 6.2 Agentic（Agent 自决写）

**开关**：`enable_agentic_memory=True`。

**机制**：

1. 主 Agent 工具列表注入 `update_user_memory(task: str)`（`_default_tools.get_update_user_memory_function`）  
2. 主模型决定是否调用、写什么 task 描述  
3. 工具内部转交 `MemoryManager.update_memory_task(task)`：  
   - 再启一轮 memory model + `add/update/delete/clear` DB 工具  
   - 真正写库发生在 **子 Agent 工具调用** 里  

State 对称：`enable_agentic_state` → `update_session_state(dict)`，改的是 `run_context.session_state`，无需第二模型。

**特点**：

- 「该不该记」由主对话模型判断，通常比 Always 更准  
- 多一次工具往返；memory 写入仍是即时落盘（**不是** Propose）  
- 适合「Agent 主动管理档案」的产品  

### 6.3 Propose（提议 + 人审，再 apply）

Agno **MemoryManager 本体默认不走 Propose**——工具 `add_memory` 直接 `db.upsert_user_memory`。Propose 能力分散在：

1. **LearningMachine 的 `LearningMode.Propose`**（概念层）：学到的内容先进 pending，HITL 批准后入库。  
2. **工具级 HITL**（`tools/function.py`）：  
   - `requires_confirmation`：执行前暂停，等用户确认  
   - `requires_user_input`：暂停要用户填字段  
   - `approval_type`：`required` / `audit`  
   - 暂停记录为 `db/schemas/approval.py` 的 `Approval`（status: pending → approved/rejected…）  
3. **外部产品层**：把 memory 写工具标成 `requires_confirmation`，即可把 Agentic 升级成「提议 → 确认 → 写」。

**语义三角**：

```text
Always   = 框架在 run 后自动写，Agent 不参与决策
Agentic  = Agent 决定写什么，写即生效
Propose  = Agent（或框架）提出意图，人批准后才生效
```

可叠加：

```text
Agentic + requires_confirmation(memory 写工具)  ≈  Propose（人审）
Always  + 外部 pending 队列                    ≈  Propose（批处理审批）
```

---

## 7. 三种模式对照（速查）

| | Always | Agentic | Propose |
|--|--------|---------|---------|
| **谁触发** | 框架（run 结束） | 主 Agent 工具调用 | Agent/框架提议 |
| **谁落盘** | MemoryManager 直接 upsert | MemoryManager 经子工具 upsert | 审批通过后 apply |
| **主模型是否知情** | 否 | 是（主动调用） | 是（提议） |
| **用户可控点** | 总开关开/关 | 对话中指令「别记这个」 | 每条批准/拒绝 |
| **额外 LLM** | 有（memory model） | 有（主 + memory） | 视实现；审批本身可不耗 LLM |
| **Agno 开关** | `update_memory_on_run` | `enable_agentic_memory` | LearningMode / tool confirmation / 产品 pending |
| **Astro 近似** | `background_review` / 入梦自动 | `memory` 工具直写 | `write_approval` → pending |

互斥提示（Agno）：`update_memory_on_run && enable_agentic_memory` 时后台 Always **不启动**，只保留 Agentic。

---

## 8. 与 Astro 的映射与建议

### 8.1 现状对照

| Agno | Astro |
|------|-------|
| `AgentSession.runs[].messages` | session 消息表 + hydrate |
| `session_data["session_state"]` | 尚无一等「会话 KV state」；可用 pinned-context / 工具侧状态近似 |
| `UserMemory` DB 行 | `MEMORY.md` / `USER.md` + daily（文件精炼） |
| `add_memories_to_context` | Frozen Snapshot 注入 system |
| `update_memory_on_run` | `auxiliary.background_review_enabled` + 入梦 |
| `enable_agentic_memory` | `memory` 工具（Agent 自写） |
| Propose | `memory.write_approval` → pending 队列 |
| `CompressionManager` | `ToolCompressionManager` + `maintain_tool_context`（已落地） |
| `Approval` / `requires_confirmation` | `ToolEntry.needs_confirmation` + HITL |

Astro **不应**为对齐而改成纯 DB 记忆；文件 + 审批是差异化。可借鉴的是 **开关正交** 与 **写入模式显式化**。

### 8.2 建议落地（概念，非本文件任务）

1. **分层读开关**（若尚未命名统一）：  
   - `inject_history` / `inject_memory` / `inject_session_state` / `inject_session_summary`  
   - 各自预算（已有 `ContextBudget` 可挂）  

2. **写入模式枚举**（对齐产品语言）：  

   ```text
   MemoryWriteMode = Always | Agentic | Propose
   ```

   - `Propose` ≡ 现有 `write_approval=true`  
   - `Agentic` ≡ `write_approval=false` + `memory` 工具可用  
   - `Always` ≡ background review / 入梦自动提炼（仍建议默认走 Propose 或仅写 daily）  

3. **State 一等公民（可选）**：  
   - 会话级 `session_state: JsonObject` 列或 sidecar  
   - 读：`add_session_state_to_context`  
   - 写：`update_session_state` 工具（可再叠 `needs_confirmation`）  

4. **禁止静默 Always 写 MEMORY/USER**：桌面场景默认 Propose；Always 仅允许写 daily 或 DecisionLog。

### 8.3 已落地相关（指向）

- `memory::protocol::{MemoryOps, propose_or_apply}` — Propose/Apply 协议  
- `memory.write_approval` — 配置级 Propose  
- `memory::decision_log` — 决策旁路（Learning 的弱化版）  
- `docs/memory.md` — 产品文档  
- `docs/context-compression.md` — Run 内压缩（对应 Agno CompressionManager）  
- `docs/agno-lessons.md` §六 / §七 — 记忆与学习总览  

---

## 9. 源码索引

| 主题 | 路径 |
|------|------|
| Session 聚合 | `session/agent.py` · `session/team.py` · `session/workflow.py` |
| Session 摘要 | `session/summary.py` |
| State 加载/合并 | `agent/_storage.py` → `load_session_state` |
| State / Memory 工具 | `agent/_default_tools.py` · `agent/_tools.py` |
| 消息组装 | `agent/_messages.py` → `get_run_messages` / system 拼装 |
| Run 管线 | `agent/_run.py` |
| Always 写记忆 | `agent/_managers.py` → `make_memories` / `astart_memory_task` |
| MemoryManager | `memory/manager.py` |
| 优化策略 | `memory/strategies/` |
| UserMemory schema | `db/schemas/memory.py` |
| Approval / HITL | `db/schemas/approval.py` · `tools/function.py`（`requires_confirmation`） |
| Tool 结果压缩 | `compression/manager.py` |
| DB upsert | `db/*/sqlite.py` 等 → `upsert_session` / `upsert_user_memory` |

---

## 10. 一句话总结

> Agno 用 **正交开关** 分别控制 History / State / Memory 的「读」与「写」；写侧再分 **Always（框架自动）**、**Agentic（Agent 工具即时落盘）**、**Propose（提议 + 人审后再 apply）**。Astro 已在文件记忆上具备 Propose（`write_approval`）与 Agentic（`memory` 工具），借鉴重点是把分层开关与写入模式 **产品化命名、互斥规则写清**，而不是换成 DB 行存储。
