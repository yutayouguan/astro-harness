# 活动卡：父折叠 + 工具调用美化（Input / Output）

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** 气泡内活动卡列表（`ActivityCards` / `MsgActivity`）与前端 `ChatActivity` 字段；不含思考块与右侧上下文时间线

## 目标

将同一条助手消息内的活动条目收入父级折叠；子项默认摘要行，展开后明确展示 **Input** / **Output** 两行分区，并按 kind 显示图标，视觉更接近常见「工具调用」样式。

## 背景与约束

- 已有逐条折叠（`MsgActivity`），verbosity=`detailed` 时默认展开详情。
- `detail` 由 `arguments_json` 与 `result` 用 `\n→\n` 拼接；不宜再靠解析箭头拆分。
- 流事件已有分开字段：`arguments_json` / `result` / memory `content`。
- Lobehub 仅 MCP 品牌图标；tool / skill / memory 用 `NavIcons`，hook / status 用 Lucide。

## 用户决策

| 项 | 选择 |
|---|---|
| 父级默认开合 | 跟 verbosity：`detailed` 开；`normal`/`compact` 关 |
| 子项 | 展开父级后只见摘要；点子项才看 Input/Output（子项默认折） |
| 图标 | 按 kind：mcp→McpIcon；tool→IconTools；skill→IconSkills；memory→IconMemory；hook→Webhook；status→Activity |
| 数据 | 前端拆 `input` / `output` 字段，不解析 `detail` |

## 方案选择

采用 **ChatActivity 增加 `input`/`output` + `ActivityGroup` 父折叠 + 美化 `MsgActivity`**，而非仅解析 `detail` 或只做父折叠不拆字段。

## UI 与交互

### 父级 `ActivityGroup`

- 折叠摘要：通用活动图标 + i18n 文案（如「工具与活动 · N」）+ chevron
- `defaultOpen = prefs.verbosity === "detailed"`；verbosity 变化时同步重置
- 展开后渲染可见子项列表
- 可见过滤仍由 `isActivityVisible` 决定；过滤后 0 条不渲染父级

### 子项 `MsgActivity`

- 摘要行：kind 图标 + `title` + running 脉冲 + chevron（有 input 或 output 或兼容 detail 时可折）
- 默认折叠；用户点击展开
- 展开区：
  - **Input** 标签 + 正文（无 `input` 则不渲染该区）
  - **Output** 标签 + 正文（无 `output` 则不渲染；仅 running 且仅有 input 时只显示 Input）
- 历史消息仅有 `detail`：整段作为 Output 兜底；可选对旧 `\n→\n` 做一次兼容拆分（仅无 input/output 时）

### 不改

- `MsgReasoning`
- `ChatContextTimeline`（可后续对齐）
- 后端流事件 DTO

## 数据与流式

### 类型（`types.ts`）

```ts
ChatActivity = {
  id: string;
  kind: ChatActivityKind;
  title: string;
  detail?: string;   // 兼容保留
  input?: string;    // 工具 arguments / 入参
  output?: string;   // 工具 result / 记忆 content
  status?: "running" | "done" | "error";
  at?: number;
};
```

### 写入（`App.tsx`）

| 事件 | input | output | detail（兼容） |
|------|-------|--------|----------------|
| `tool_call_delta` | 累积 args | — | = input |
| `tool_call` | `arguments_json` | `result` | 可继续拼 `\n→\n` 或仅保留字段 |
| `memory_update` | — | `content` | = content |

## 组件与文件

| 文件 | 职责 |
|------|------|
| Create/改 `MsgActivity.tsx` | 图标 + Input/Output 分区 |
| Create `ActivityGroup.tsx`（或内联于 ChatView） | 父折叠容器 |
| 改 `ChatView.tsx` `ActivityCards` | 过滤后包 `ActivityGroup` |
| 改 `types.ts` | `input` / `output` |
| 改 `App.tsx` | 流式写入 |
| 改 `chat.css` | 父级/子项/分区样式 |
| 改 `messages.ts` | 父级摘要、Input/Output、aria 文案 |

推荐抽出 `ActivityGroup` 以保持与 `MsgReasoning` / `MsgActivity` 对称，避免 `ChatView` 继续膨胀。

## 边界情况

- 同一消息多张卡：父级统一包一层；子项独立 `open`
- verbosity 切换：父级按新默认重置；子项保持「默认折」策略（随 `defaultOpen` 同步时子项重置为关，除非另定）
- 流式：先出现 Input，完成后补 Output；running 脉冲在摘要行
- compact：不可见的 kind 不计入 N；父级只包可见项

## 验收

1. normal：父级默认折；展开后子项默认折；点子项可见 Input/Output 分区
2. detailed：父级默认开；子项仍默认折，可手开
3. kind 图标正确（含 MCP lobehub）
4. 流式过程先 Input 后 Output
5. 思考块与右侧时间线行为不变
6. 仅有旧 `detail` 的历史消息仍可展开查看内容

## 非目标

- 不改后端事件 schema
- 不合并 tool 卡与 memory_update 为单条（仍可能双卡）
- 不做 LobeChat 像素级复刻，只借鉴「工具调用」分区与层级折叠
