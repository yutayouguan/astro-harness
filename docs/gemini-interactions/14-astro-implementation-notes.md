# Astro 实现备注：Interactions 工具调用与 Thought

> 本文记录 Astro 对接 Gemini **Interactions API**（Api-Revision `2026-05-20`）时的线上 wire 差异、根因与落地约定。  
> 官方镜像见同目录其它章节；本文以**本仓库代码行为**为准。

抓取 / 修复对照时间：2026-07-17。

---

## 1 问题现象

使用 Google 模型做工具调用时，工具 **INPUT 恒为 `{}`**（例如 `image_gen` 报 `missing field 'prompt'`）。

表面像「模型没给参数」；实际是 SSE 增量字段与解析假设不一致，参数被丢弃。

---

## 2 线上 Wire（抓包结论）

真实流式事件大致如下（工具名/签名已缩短）：

```text
event: step.start
data: {"index":0,"step":{"type":"function_call","id":"…","name":"get_weather","arguments":{},"signature":"…"},"event_type":"step.start"}

event: step.delta
data: {"index":0,"delta":{"type":"arguments_delta","arguments":"{\"location\":\"Paris\"}"},"event_type":"step.delta"}

event: interaction.completed
data: {"interaction":{"id":"…","status":"requires_action","usage":{"total_input_tokens":66,"total_output_tokens":16,"total_thought_tokens":51,…}},"event_type":"interaction.completed"}
```

要点：

| 项目 | 官方文档示例 | 线上实际 |
| --- | --- | --- |
| 参数增量 | `delta.type=arguments` + `partial_arguments` | `delta.type=arguments_delta` + `arguments`（字符串） |
| `step.start` 上的 `arguments` | 可能已有完整对象 | 常为**空对象** `{}`；真参在后续 delta |
| `function_call.signature` | SDK 自动处理 | step 上常带非空 signature；无状态回放必须原样回传 |
| 结束状态 | 文档混用多种事件 | 工具暂停多为 `interaction.completed` + `status=requires_action` |
| Usage | `prompt_tokens` 等旧名 | `total_input_tokens` / `total_output_tokens` / `total_thought_tokens` / `total_cached_tokens` |
| Thought | `thought` + `thought_signature` | 可能先占 step index 0；function_call 在后续 index |

Gemini 3 默认思考时，常见顺序是：`thought`（index 0）→ `function_call`（index 1）。  
`arguments_delta` 的顶层 `index` 有时仍指向 thought 的 index，**不能**按顶层 index 新建无名 tool 槽。

---

## 3 Astro 修复清单

### 3.1 工具入参为空 `{}`（主 bug）

**文件**：`crates/agent-providers/src/google/interactions_chat.rs`

- 同时识别 `arguments_delta` + `arguments` 与 `arguments` + `partial_arguments`
- `step.start` 上的空 `{}` 不当作有效参数
- 用 `current_fc_slot` / `arguments_slot()` 把参数增量绑到**当前** `function_call` 槽，避免与 thought 的 index 错位

### 3.2 `function_call.signature` 全链路

流式 `step.start` → `ToolCallDelta.signature` → `ParsedToolCall` / `common::ToolCall.signature` → 会话落盘 → `ChatToolCall` → `messages_to_interactions_input` 回写 `function_call.signature`。

无状态或丢弃 `previous_interaction_id`、回退全量历史时，缺少 signature 会触发 Gemini 3 严格模式错误。

### 3.3 `thought` / `thought_signature`

| 阶段 | 行为 |
| --- | --- |
| SSE | 解析 `step.delta` 的 `thought_signature`，以及 `step.start` 上带 `signature` 的 `thought` |
| 流事件 | `ChatChunk.thought_signature` → `StreamedAssistantContent::ThoughtSignature`（不转发到 UI gRPC） |
| 落盘 | 写入 `reasoning_details.google_thought_signature`；`Message.reasoning` / `thought_signature` 进会话镜像 |
| Hydrate | 从 DB `reasoning` + `reasoning_details` 还原 |
| 回放 | `encode_thought_step`：在 `function_call` **之前**插入 `thought`（signature ± reasoning 文本） |

有状态主路径（`previous_interaction_id` + 默认 `store=true`）只提交尾部 `function_result` / `user_input`，**由服务端保留** thought/signature；客户端持久化是为无状态与全量回退兜底。

### 3.4 其它对齐

- **Usage**：认 `total_*_tokens`，兼容旧命名
- **有状态多轮**：tool 续写只发增量 `function_result`；user 续写只发尾部 `user_input`；`system_instruction` / `tools` / `generation_config` 仍重传（interaction-scoped）
- **finish_reason**：按 `interaction.status`（`requires_action` / `failed` / `incomplete`）映射
- **Api-Revision**：媒体与聊天路径统一 `2026-05-20`

---

## 4 有状态 vs 无状态（Astro 约定）

```text
默认（推荐）
  store=true + previous_interaction_id
  └─ 工具多轮：只提交本轮 function_result；服务端保留 thought / FC signature

回退 / store=false
  全量 messages → input
  └─ 必须按序重放：user_input → thought? → model_output? → function_call(+signature) → function_result → …
```

注入 `previous_interaction_id`：`crates/agent-core/src/streaming/provider.rs`（流中捕获 `interaction.id`，下一轮写入 `ProviderConfig`）。

---

## 5 关键代码路径

| 职责 | 路径 |
| --- | --- |
| SSE 解析 / 请求体 | `crates/agent-providers/src/google/interactions_chat.rs` |
| HTTP + Api-Revision | `crates/agent-providers/src/google/interactions_http.rs` |
| Chunk / ChatMessage 字段 | `crates/agent-providers/src/api/trait_.rs` |
| 工具 delta 累积 | `crates/agent-tools/src/engine/parse.rs` |
| 多轮流 + 落盘 | `crates/agent-core/src/streaming/multi_turn.rs` |
| 会话镜像 / hydrate | `common/src/message.rs`、`crates/agent-core/src/runtime/{mod,session}.rs`、`crates/agent-core/src/prompt/messages.rs` |
| gRPC（忽略 ThoughtSignature） | `crates/agent-server/src/grpc/astro_service.rs` |

相关 helper：

- `common::message::GOOGLE_THOUGHT_SIGNATURE_KEY`
- `merge_google_thought_signature` / `google_thought_signature_from_details`

---

## 6 相关提交（摘要）

| 主题 | 说明 |
| --- | --- |
| arguments 槽绑定 | 避免 thought index 拆散 name/arguments |
| `arguments_delta` 解析 | 修复工具入参恒为 `{}` |
| Usage / 有状态 / finish_reason | 对齐线上 wire |
| `function_call.signature` 全链路 | 无状态回放必需 |
| `thought_signature` 捕获与回放 | `a4a47a9` 等 |

具体 hash 以 `git log --grep=Gemini` / `--grep=Interactions` 为准。

---

## 7 验证清单

1. 重编后端 / `tauri dev`
2. Google 模型调用 `image_gen`（或任意带必填参数的工具）：INPUT 应含真实 JSON，而非 `{}`
3. 同会话多轮 tool 续写：有 `previous_interaction_id` 时不应因缺 thought 而 400
4. （可选）强制全量历史回退路径：确认 `input` 中 `thought` 在 `function_call` 之前，且两者 signature 非空

单测入口：

```bash
cargo test -p providers google::interactions_chat
```

覆盖：`arguments_delta`、FC signature 回放、thought 回放、`thought_signature` delta、usage 字段等。

---

## 8 与官方镜像文档的交叉引用

- 迁移 SSE 示例：[01-migrate-to-interactions.md](./01-migrate-to-interactions.md)（文末 Wire 说明）
- 函数调用流式：[09-function-calling.md](./09-function-calling.md) §1.12 实现备注
- 无状态须原样回传 thought / function_call：[02-get-started.md](./02-get-started.md) §1.4.5

官方文档示例字段名可能滞后于线上；**实现以本文与抓包为准，并兼容文档旧字段**。

---

## 9 数据库落库形态与跨模型切换

Astro 的会话历史统一写入 `messages` 表；不会按 Provider 拆成 Google 表 / DeepSeek 表。跨模型切换时，两者共享同一套 `role` / `content` / `tool_calls` / `reasoning` / `reasoning_details` 等列，差别只在部分 JSON 字段是否存在。

### 9.1 共用列

| 列 | 含义 |
| --- | --- |
| `role` | `user` / `assistant` / `tool` |
| `content` | 可见正文；纯工具调用的 assistant 可为空 |
| `tool_calls` | assistant 发起的工具调用 JSON |
| `tool_call_id` / `tool_name` | tool 结果关联字段 |
| `reasoning` | 流式累积的思考文本（若 Provider 下发） |
| `reasoning_details` | Astro 时间线、surface，以及 Google thought signature 等结构化 JSON |
| `reasoning_content` | 兼容列；当前主路径基本不写，通常为 `NULL` |
| `media_json` | 结构化媒体附件（如图片生成结果） |

### 9.2 Google 工具调用行

Google Interactions 工具调用的 assistant 行会额外保留：

- `tool_calls[].signature`：`function_call.signature`，无状态回放必须原样回传。
- `reasoning_details.google_thought_signature`：`thought.signature`，用于全量历史回放时重建 `thought` step。

示例：

```json
{
  "role": "assistant",
  "content": "",
  "reasoning": "用户要生成一张猫的图片…",
  "tool_calls": [
    {
      "id": "sf9vftls",
      "name": "image_gen",
      "arguments": { "prompt": "a cat" },
      "signature": "Eq0CCqoCARFNMg9…"
    }
  ],
  "reasoning_details": {
    "astro_timeline_v1": [
      { "type": "reasoning", "text": "用户要生成一张猫的图片…", "at": 1721 },
      { "type": "activity", "id": "sf9vftls", "at": 1722 }
    ],
    "google_thought_signature": "EvEFCu4F…"
  }
}
```

对应 tool 行仍是通用形态：

```json
{
  "role": "tool",
  "tool_call_id": "sf9vftls",
  "tool_name": "image_gen",
  "content": "…生成结果…",
  "media_json": "[{\"kind\":\"image\",…}]"
}
```

### 9.3 DeepSeek 工具调用行

DeepSeek / OpenAI 兼容路径使用标准工具调用字段，不写 Google 专用 signature：

```json
{
  "role": "assistant",
  "content": "",
  "reasoning": "需要调用 image_gen…",
  "tool_calls": [
    {
      "id": "call_abc123",
      "name": "image_gen",
      "arguments": { "prompt": "a cat" }
    }
  ],
  "reasoning_details": {
    "astro_timeline_v1": [
      { "type": "reasoning", "text": "需要调用 image_gen…", "at": 1721 },
      { "type": "activity", "id": "call_abc123", "at": 1722 }
    ]
  }
}
```

要点：

- DeepSeek 行通常没有 `tool_calls[].signature`。
- DeepSeek 行通常没有 `reasoning_details.google_thought_signature`。
- `reasoning` / `astro_timeline_v1` 是 Astro 通用字段，DeepSeek thinking 模型也可以写入。

### 9.4 跨模型切换行为

| 切换 | 行为 |
| --- | --- |
| Google → DeepSeek | DeepSeek 请求体只编码标准 `tool_calls`，不会带 Google 的 `signature` / `google_thought_signature`。 |
| DeepSeek → Google | 没有 `previous_interaction_id` 时走全量历史回放；Google 旧行上的 signature / thought signature 可用于重建 Interactions steps。 |
| 同会话混用 | 数据库中会交替出现两类行；它们共享同一张 `messages` 表。 |

注意：工具确认卡（HITL）仍在等待、或流式响应尚未结束时，不建议中途切模型；应先完成当前工具轮，避免悬挂 `tool_calls`。
