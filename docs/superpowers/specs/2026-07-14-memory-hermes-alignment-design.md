# 记忆系统对齐升级设计

**日期:** 2026-07-14  
**状态:** 已实现（P1 + P2：含 write_approval、auxiliary/dreaming、回合后 background review 挂接、活会话 REFRESH_MEMORY）  
**范围:** 有界精炼记忆（MEMORY/USER）、Frozen Snapshot、单一 `memory` 工具、安全扫描、`write_approval`、回合后 review、入梦 auxiliary 模型  
**外部参考（仅设计借鉴，不引入其品牌命名）：** [Persistent Memory 文档](https://hermes-agent.nousresearch.com/docs/user-guide/features/memory)

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`MemoryStore`、`FrozenMemorySnapshot`、`memory` 工具、`write_approval`、`auxiliary.dreaming`、`auxiliary.background_review`。
- 本文档仅在「外部参考」处提及上游项目；正文用「参考记忆架构」表述。

## 目标

将 Astro 记忆从「Markdown 列表 + 超限 FIFO 淘汰 + 每轮重读盘」升级为：

1. **有界精炼记忆**：`MEMORY.md` / `USER.md`，默认紧字符上限，超限报错由 Agent consolidate  
2. **Frozen Snapshot**：同会话 system prompt 中的记忆块冻结，稳住 prefix cache  
3. **单一 `memory` 工具**：`action` + `target`，无 `read`，子串定位  
4. **安全门禁**：精确去重 + 写入前扫描（injection / 外泄 / 不可见 Unicode）  
5. **第二轨不变**：`session_search` + SessionStore FTS  
6. **保留入梦**：日记仍为原料；入梦与 background review 走 **便宜 auxiliary 模型**  
7. **可选 `write_approval`**：含回合后自动 review 的写入门禁  

## 决策摘要（已定）

| 项 | 选择 |
|----|------|
| 落地节奏 | **两阶段**：P1 内核对齐 → P2 学习闭环 |
| 工具 | 单一 `memory`；废除 `memory_add` / `memory_replace` / `memory_remove` |
| 字符上限 | 默认 `2200` / `1375`，`config.yaml` 可配 |
| Snapshot | 同会话冻结；新 `session_id` 或显式 `refresh_memory` 重载 |
| 日记 | **不进** `memory` 工具；系统/入梦管线写 `mermaid/` |
| 文件格式 | 读兼容 `- ` / `*` / `§`；**新写入统一 `§`** |
| 审批 + review | P2：`write_approval` + 回合后 cheap-model review |
| 外部 provider | **不做** |

## 分期

### P1（内核）

- `MemoryFile` → `MemoryStore`（双文件、双态 live/snapshot、§、超限报错、扫描、去重）  
- `AgentLoop` Frozen Snapshot 注入  
- 单一 `memory` 工具 + `session_search` 保留  
- 日记退出 tool；今日日记 prompt **截断**注入  
- `config.yaml`：`memory.memory_char_limit` / `user_char_limit` / enabled 开关 / `daily_prompt_max_chars`  
- 入梦写回 MEMORY **改走** MemoryStore 门禁（模型路由可仍暂用现有；auxiliary 字段可先解析占位）  

### P2（学习闭环）

- `memory.write_approval` + pending 队列 + approve/reject（Tauri/设置最小 UI）  
- 回合后 `auxiliary.background_review`  
- 入梦强制 `auxiliary.dreaming`  
- 可选聊天通知「记忆已更新」  

### 明确不做

- Mem0 / Honcho 等外部 memory provider  
- 改 SessionStore schema  
- 日记作为 `memory` 的 `target`  
- 会话中途自动刷新 snapshot  
- 自动 skill patch（参考架构的 skill write_approval 不在本期）  

---

## 架构

```text
config.yaml (memory.* / auxiliary.*)
        │
        ▼
 MemoryStore ── live + snapshot ── MEMORY.md / USER.md
        ▲
        │
 MemoryManager ── SessionStore (FTS) ── session_search
        │         └── mermaid/ 日记（系统/入梦写）
        ▼
 AgentLoop ── StaticContext ← snapshot only
           └── turn end ──(P2)── background review ──► MemoryStore
 Dreaming  ──(P2 aux)── Extractor LLM ──► MemoryStore
```

| 组件 | 职责 | 建议路径 |
|------|------|----------|
| `MemoryStore` | 条目解析/序列化、容量、扫描、去重、live/snapshot | 演进 `memory/src/agent/files.rs` 或新建 `memory/src/agent/store.rs` |
| `MemoryManager` | 聚合、工具分发、`refresh_snapshot`、日记 append API（非工具） | `memory/src/session/manager.rs` |
| `AgentLoop` | 捕获/使用 snapshot；构造与换 session 时 reload | `agent/src/loop_.rs` |
| Tool 注册 | `memory` + `session_search` | `tools/src/builtins/memory_tools.rs` |
| Config | 扩展共享 `AstroConfig`（hooks 继续读 `hooks:`） | `hooks/src/config.rs` 或抽 `astro-config` 薄模块；**禁止**两套 yaml |

---

## MemoryStore 契约

### 文件与条目

- 路径：`{workspace}/MEMORY.md`、`{workspace}/USER.md`（不变）  
- Canonical 分隔：`\n§\n`（条目可多行）  
- **读**：识别 `§` 分隔；若整文件无 `§` 且存在 `- `/`* ` 列表行，则按旧列表解析  
- **写**：成功 save 后盘上统一为 `§` 格式（渐进迁移）  

### 容量

| Store | 默认上限 | 配置键 |
|-------|----------|--------|
| memory | 2200 chars | `memory.memory_char_limit` |
| user | 1375 chars | `memory.user_char_limit` |

- 计数模型无关（字符，含分隔开销；实现中固定一种与参考架构一致的计量方式并单测钉死）  
- `add` / 会使总量上升的 `replace`：**超限失败**，**禁止** FIFO 淘汰  
- 错误须包含：`usage`、`current_entries`、consolidate 提示  

### 双态

- `live`：工具 / 入梦 / review 变更后立即反映到内存与磁盘  
- `snapshot`：仅 `load` / `refresh_memory` / 新会话绑定时更新；供 system prompt  

### 操作语义

| 操作 | 规则 |
|------|------|
| `add` | trim；空失败；与已有**精确相等** → 成功且不追加 |
| `replace` | `old_text` 子串；0 或 >1 匹配失败；新内容受上限与扫描约束 |
| `remove` | 同子串唯一匹配；无 `read` |

### 安全扫描（写入前）

拒绝：

- 选定不可见 Unicode（如 U+200B–U+200D、BOM、bidi 覆盖等）  
- 轻量威胁模式：prompt injection、凭据外泄（curl/wget + KEY/TOKEN）、SSH/`authorized_keys`、读取 `.env` 等（Rust 实现与参考列表同级，可后续增补）  

命中 → 直接失败，**不**写入、**不**入 pending。

### Prompt 渲染（snapshot）

须含 store 名、用量百分比与 `used/limit`；条目以 `§` 连接。动态召回（FTS）仍走现有 `DynamicContext`，与 snapshot 无关。

### 今日日记

- 路径：`mermaid/YYYY-MM-DD.md`  
- **不**进 `memory` 工具  
- 可被系统侧 `append_daily`、background review「写日记」意图、入梦读取  
- 注入 prompt：每轮可读盘，但截断至 `memory.daily_prompt_max_chars`（默认 **1024**），并标明非长期记忆  

### 旧文件超默认上限

- load **允许**展示（snapshot/live 均可大于 limit）  
- 任何使用量升高的写入失败，直到 consolidate 或用户调高 limit  

---

## 工具契约

### `memory`（P1 起唯一记忆写入工具）

```json
{
  "name": "memory",
  "parameters": {
    "action": "add | replace | remove",
    "target": "memory | user",
    "content": "string?",
    "old_text": "string?"
  }
}
```

- `target=memory` → MEMORY.md；`target=user` → USER.md（对外 JSON 不用 `project`）  
- 工具回包装 **live** + usage；注明当前会话 snapshot 未刷新  
- 旧名 `memory_add` / `memory_replace` / `memory_remove`：**取消注册**；若 dispatch 仍收到旧名，返回明确迁移错误  

### `session_search`

保持独立工具与现有 FTS 行为；第二轨「全历史按需检索」。

### 内部类型

- `MemoryTarget`：`Memory` | `User`（删除 `Daily` / `Project` 对外枚举）  
- 日记 API：`MemoryManager::append_daily` 等保留为 **非工具** 方法  

---

## Frozen Snapshot 生命周期

| 事件 | Snapshot |
|------|----------|
| `AgentLoop` 构造 / `with_session_id` | load → 固化 |
| `run_turn` → `build_system_prompt` | **只读 snapshot** 填 MEMORY/USER |
| `memory` 工具成功 | live+盘更新；snapshot **不变** |
| 新/切换 `session_id` | reload |
| `refresh_memory`（Rust + Tauri） | reload |
| 切换 Agent workspace | 新 Manager，必 reload |

---

## 配置

扩展 `~/.astro/config.yaml`（或 `ASTRO_MEMORY_DIR/config.yaml`）：

```yaml
memory:
  memory_enabled: true
  user_profile_enabled: true
  memory_char_limit: 2200
  user_char_limit: 1375
  write_approval: false
  daily_prompt_max_chars: 1024

auxiliary:
  background_review:
    provider: auto
    model: auto
  dreaming:
    provider: auto
    model: auto
```

- `memory_enabled: false`：不注入 MEMORY、禁用 `memory` 工具写 MEMORY（USER 由 `user_profile_enabled` 对称控制）  
- `provider`/`model` 为 `auto`：跟随当前会话主模型；非 auto 时强制该路由  
- 与现有 hooks 配置合并加载，单一文件  

---

## P2：write_approval 与 pending

- `write_approval: false`：过 Store 门禁后直接落盘  
- `true`：MEMORY/USER 写入（工具、review、入梦）进入 `~/.astro/pending/memory/*.json`；approve 后应用，reject 丢弃  
- 日记 **不受**审批门禁  
- 扫描失败不入队  
- UI：设置页 / 记忆页最小 pending 列表 + 批准/拒绝；slash 命令可后续补  

## P2：background review

- 主 turn **成功结束后异步**触发，不阻塞下一用户消息  
- 模型：`auxiliary.background_review`  
- 输入：近回合 digest（最近 N 轮原文 + 更早摘要）；review 模型 ≠ 主模型时必须用 digest  
- 输出：建议的 memory add/replace/remove；可选一行写入今日日记  
- 应用路径与工具写入相同（含审批）  
- **不做**自动 skill 修改  

## 入梦

- 保留 `dreaming.json`、选未做梦日记、finalize 流程  
- Extractor 使用 `auxiliary.dreaming`（便宜模型）  
- 写回 MEMORY 必须经 MemoryStore；超限/扫描失败 → finalize 失败并记日志，**禁止**静默截断  
- 入梦成功不自动 refresh 当前会话 snapshot  

---

## 数据流（一轮）

```text
session 打开 / refresh_memory
  → load → snapshot(MEMORY, USER)

用户消息
  → 近期 + 可选 FTS（动态）
  → system = SOUL + snapshot + 截断今日日记
  → LLM
  → memory(...) → scan → [P2 pending?] → live 写盘
  →（P2）turn end → auxiliary review → 同上
  →（入梦任务）auxiliary dreaming → MemoryStore
```

---

## 错误与兼容

| 情况 | 行为 |
|------|------|
| 超限 | 失败 + usage + entries + consolidate 提示 |
| 扫描拦截 | 失败 + pattern id |
| 子串 0/>1 | 失败 |
| 精确重复 add | 成功，no duplicate |
| 旧三工具名 | 迁移错误文案 |
| 旧列表文件 | 读兼容；首次 save 转 `§` |

---

## 测试要求

1. Store：§ 读写、旧列表兼容、超限不淘汰、去重、歧义子串、扫描  
2. Loop：同 session 写入后 prompt 仍旧 snapshot；refresh / 换 session 后更新  
3. 工具注册：仅 `memory` + `session_search`；旧名拒绝  
4. 入梦：超限失败可观测  
5. P2：approval on 时未 approve 不落盘；review mock provider  

---

## 验收标准

**P1**

- [x] 默认上限 2200/1375 可配置  
- [x] 超限报错、无 FIFO  
- [x] Frozen Snapshot 行为符合上表  
- [x] 单一 `memory` 工具；无 daily target  
- [x] 写入扫描 + 精确去重  
- [x] 代码与 UI 无参考项目品牌名  

**P2**

- [x] `write_approval` + pending 闭环  
- [x] 回合后 review 可开关/可配模型  
- [x] 入梦走 `auxiliary.dreaming`  
- [x] 入梦/review 写 MEMORY 遵守 Store 门禁  

---

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 旧 MEMORY 远超 2200 | load 允许；写入失败引导 consolidate / 调 limit |
| Prefix cache vs 日记每轮变化 | 日记截断且与 MEMORY 分层；可选后续将日记移出静态前缀 |
| 双格式读写 bug | 单测矩阵：纯列表 / 纯 § / 混合 |
| 审批 UX 简陋 | P2 最小列表即可；消息平台 slash 后补 |
| Config 分裂 | 单一 `config.yaml`，扩展现有加载 |

---

## 实现顺序建议（供 writing-plans）

1. `MemoryStore` + 单测  
2. `MemoryManager` / dispatch 切新工具契约  
3. `AgentLoop` snapshot + Tauri `refresh_memory`  
4. tools 注册表与 `tools_enabled` / delegate 名单  
5. config 解析  
6. 入梦走 Store 门禁  
7. P2：pending + approval  
8. P2：background review + auxiliary dreaming 路由  

## 参考

- 会话存储：[`2026-07-13-session-store-design.md`](./2026-07-13-session-store-design.md)  
- 外部：Persistent Memory（见文首链接）  
