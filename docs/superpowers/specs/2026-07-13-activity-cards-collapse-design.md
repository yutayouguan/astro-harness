# 聊天活动卡：详情默认折叠

**日期:** 2026-07-13  
**状态:** 已批准 / 已实现  
**范围:** 气泡内工具 / 记忆等活动卡（`ActivityCards`）；不含思考块与右侧上下文时间线

## 目标

气泡内活动卡默认不抢视线：摘要一行可见，详情可点开查看。思考区（`MsgReasoning`）保持现有流式展开、结束后自动折叠行为。

## 背景与约束

- 消息模型为扁平字段：`content`、`reasoning`、`activities[]`，无独立 message-parts 模型。
- `ActivityCards`（`ChatView.tsx`）当前始终展示 kind + title；`detail` 仅在 `verbosity === "detailed"` 时直接渲染，卡片本身不可折叠。
- `MsgReasoning` 已有折叠交互，可作为组件模式参考。
- 右侧 `ChatContextTimeline` 已有单条展开，本次不改。
- 活动可见性仍由 `useChatDisplayPrefs`（`showTools` / `showMemory` 等）与 `isActivityVisible` 控制。

## 用户决策

| 项 | 选择 |
|---|---|
| 范围 | 只改工具/活动卡；思考保持现状 |
| 粒度 | 逐条折叠（每张卡独立） |
| 与 verbosity | normal/compact 默认折叠；detailed 默认展开；仍可手动切换 |

## 方案选择

采用 **抽出 `MsgActivity` 组件**（对齐 `MsgReasoning`），而非在 `ActivityCards` 内联大段状态，也非原生 `<details>`（难与 verbosity 默认及现有样式统一）。

## UI 与交互

### 折叠态

- 显示：`kind` 标签 + `title`
- running 时保留现有脉冲指示
- 有 `detail` 时可点击标题行（或 chevron）展开；`aria-expanded` 标明状态
- 无 `detail`：不可折叠，仅摘要行

### 展开态

- 显示现有 `detail`（args / `→` / result）
- 时间戳仍由 `showTimestamps` 控制，逻辑不变

### 默认开合

| verbosity | 有 `detail` 时默认 |
|---|---|
| `compact` / `normal` | 折叠 |
| `detailed` | 展开 |

## 组件与数据流

### 新增 `frontend/src/components/MsgActivity.tsx`

- Props：`activity: ChatActivity`、`defaultOpen: boolean`、`showTimestamp: boolean`
- 本地 `open`；有 `detail` 时用 button 切换
- `useEffect`：当 `defaultOpen` 变化时同步 `open`（切换 verbosity 时批量重置；覆盖此前手动状态）

### 改 `ActivityCards`（`ChatView.tsx`）

- 可见性过滤不变（`isActivityVisible`）
- 渲染 `<MsgActivity defaultOpen={prefs.verbosity === "detailed"} showTimestamp={…} />`

### 样式（`chat.css`）

- 在现有 `.msg-activity*` 上增加 toggle / chevron / `is-open`，延续当前卡片视觉

### i18n

- 可见文案仍用 kind + title
- 需要时为折叠按钮提供简短 `aria-label`（展开/折叠详情）

## 边界情况

- 流式中途补上 `detail`：一旦有 detail，按当前 verbosity 默认开合；running 指示不受折叠影响
- 同一消息多张卡：各自独立开合
- 切换 verbosity：所有卡按新默认重置（detailed→开，其余→关）
- compact：不可见的卡本来不渲染；可见卡按非 detailed → 默认折叠处理

## 非目标

- 不改 `MsgReasoning`
- 不改 `ChatContextTimeline`
- 不改后端流事件契约
- 不把活动卡合并成「工具调用 (N)」整组折叠

## 验收

1. normal：活动卡默认一行摘要；点开可见完整 detail
2. detailed：默认展开 detail；仍可手动折叠
3. 无 detail 的卡无折叠控件
4. 思考块行为与改前一致
5. 切换 verbosity 后，默认开合随之更新

## 测试

手动验证即可。无新后端契约；除非仓库已有同类组件测，否则不强制新增单测。
