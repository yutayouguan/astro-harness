# 上下文用量 Explorer 设计

**日期:** 2026-07-15  
**状态:** 已实现  
**范围:** Composer 上下文浮层 + 右栏「上下文」Tab 的用量 Explorer；Agent 组装时分层 token 快照  
**参考:** Cursor Context Usage 弹层、Context Explorer 页（视觉语言映射到 Astro 主题变量）

## 目标

让用户一眼看清本轮上下文占用比例与分层构成（系统 / 工具 / MCP / 记忆 / Skills / 召回 / 子 Agent / 对话），入口贴近 Composer，详情落在右栏「上下文」Tab；数字与真实组装内容同源，主题与现有玻璃风一致。

## 用户决策摘要

| 决策项 | 选择 |
|--------|------|
| 入口形态 | Composer 浮层 + 右栏详情（两者兼顾） |
| 数据深度 | 后端真实分层统计（组装时落盘） |
| 分项粒度 | Astro 合并档，并单独拆出 MCP、子 Agent 返回 |
| 「查看报告」 | 浮层「查看详情」→ 打开右栏并切到「上下文」Tab |
| 右栏结构 | 上半 Context Explorer（环形图 + 可展开列表），下半保留活动时间线（S1） |
| 实现路径 | 方案 1：组装时落盘快照，前端只读 |
| 估算口径 | `ceil(chars / 4)`；文案带 `~`；窗口用模型 `context_window`，缺省 128K |

## 分项类别（8）

| id | 标签 | 计入内容 | 主题色（建议） |
|----|------|----------|----------------|
| `system` | 系统提示 | SOUL / IDENTITY / AGENT / 工具指引 / 时间戳等非记忆 system 层 | `--ink-mute` / slate |
| `tools` | 工具定义 | 内置 toolset 的 API schema（非 `mcp__`） | `--tone-purple` |
| `mcp` | MCP 与动态工具 | 名称以 `mcp__` 开头的工具 schema | `--tone-pink` |
| `memory` | 记忆与画像 | MEMORY / 用户画像 / 今日记忆等 | `--tone-green` |
| `skills` | Skills | Skills 索引层 | `--tone-amber` |
| `recall` | 动态召回 | DynamicContext / 召回对话块 | `--tone-cyan` |
| `subagent` | 子 Agent 返回 | `delegate` / 编排子任务写回的 tool result 文本 | `--tone-blue` / `--tone-indigo` |
| `conversation` | 对话消息 | 用户/助手正文 + 非委派类工具结果 | `--tone-orange` |

- 分项为 **0** 时默认 **不渲染** 列表行，环形/分段条也不留空段。
- `meta.count` 可选：工具数、MCP 数、Skills 数、子任务数、消息数等，供括号展示。
- 本轮不单独统计「子 Agent 定义/人设注入」；若未来需要再加类别。

## 数据模型

```ts
type ContextUsageSegmentId =
  | "system"
  | "tools"
  | "mcp"
  | "memory"
  | "skills"
  | "recall"
  | "subagent"
  | "conversation";

type ContextUsageSnapshot = {
  contextWindow: number;
  totalTokens: number; // 各 segment 之和（估算）
  segments: Array<{
    id: ContextUsageSegmentId;
    tokens: number;
    meta?: { count?: number };
  }>;
  updatedAt: number;
};
```

Rust 侧使用等价结构（serde 字段建议 `snake_case` 与前端 camelCase 约定对齐项目惯例）。

## 后端行为

1. 在 Agent **组装本轮请求**（system 分层、tools schema、messages）时，按上表归类累计字符数。
2. `tokens = ceil(chars / 4)`，写入本轮 `ContextUsageSnapshot`。
3. 经现有聊天事件流或 Tauri 状态推到前端（与 session/turn 关联）。
4. Provider 返回的 `usage` **不覆盖**分层明细；仅可用于总量旁证或日志，分层以本地估算为准。
5. 归类规则单元测试必覆盖：`mcp__` 前缀 → `mcp`；`delegate`（及等价委派）tool result → `subagent`。

## 前端 UI

### Composer 浮层（参考 Context Usage 弹层）

- 触发：输入区旁上下文 % 按钮。
- 内容：标题「上下文使用」、`N% 已用`、`~已用 / 上限`、多色分段条、八类简表（隐藏 0）。
- 动作：「查看详情」打开右栏并 `tab = context`；关闭按钮。
- 主题：`--menu-glass-*` / `--glass-panel` 等现有浮层变量，不用参考图纯炭灰硬编码。

### 右栏「上下文」Tab（参考 Context Explorer）

自上而下：

1. **指标行**：会话或工作区标识、上下文窗口、已用 tokens  
2. **短说明** + **环形图**（中心 `N% Full` /「已用」）  
3. **可展开分项列表**（按 tokens 降序；「展开全部」）  
4. **现有 `ChatContextTimeline`**（活动记录，S1 保留）

不做：「Debug with Agent」按钮、跳转「数据洞察」。

### Agent Tab

用量详情以 Explorer 为准；Agent 信息区弱化为摘要或「在上下文中查看」链接，避免双份完整列表。

### 共享组件

| 组件 | 职责 |
|------|------|
| `ContextUsageBar` | 分段进度条 |
| `ContextUsagePopover` | Composer 浮层 |
| `ContextExplorer` | 右栏用量区（指标、环图、列表） |
| `ChatContextTimeline` | 不变，挂在 Explorer 下方 |

## 空态与降级

| 情况 | 行为 |
|------|------|
| 尚无快照 | Composer 不显示 %（或灰显）；浮层/Explorer 说明「发送对话后可查看」；时间线照常 |
| 上一份可用、本轮未到 | 保留上一份快照 |
| `context_window` 未知 | 回落 128K；可标「估算」 |
| 组装失败 | 不捏造分项；回落空态或上一份 |

## 测试

- **后端：** 分层归类与 `ceil(chars/4)` 聚合单测（MCP、delegate、记忆层归并）。
- **前端：** token 短格式（如 `9.8K`）、占用 %、隐藏 0 项、「查看详情」切 Tab。
- **不做本轮：** 全链路 E2E、真实 tokenizer、Insights 深链。

## 范围外

- Debug with Agent  
- 真实 BPE/tiktoken  
- 用 provider `usage` 回填分层  
- 替换或移除活动时间线  
- 子 Agent 定义类单独分项  

## 验收标准

1. 有快照时，Composer % 与 Explorer 环形中心百分比一致（相对同一 `contextWindow`）。  
2. 分段条/环/列表三类视觉的颜色与 `id` 一一对应。  
3. 启用 MCP 与完成至少一次 delegate 后，快照中分别出现 `mcp`、`subagent`（非 0）。  
4. 「查看详情」打开右栏「上下文」Tab。  
5. 亮/暗主题均使用设计 token，无硬编码参考图灰底。
