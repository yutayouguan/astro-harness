# 共享启发式浮层定位（clampPopover）

日期：2026-07-15  
状态：阶段 A/B/C 已实现（见 [实现计划](../plans/2026-07-15-clamp-popover.md)）

## 背景

聊天等界面里，多处下拉/浮层用 `position: absolute` 挂在 trigger 下。父级（尤其 `.content-pane`）常有 `overflow: hidden`，贴窗边或变宽（如模型编辑侧栏）时会被裁切。

现状不统一：

| 组件 | 现状 |
|------|------|
| `SelectMenu` | portal + 视口钳制 + 上下翻转 |
| `AgentPicker` | 就地 absolute + 视口水平钳制 |
| `ModelPicker` | 就地 absolute + content-pane 水平钳制；编辑态 `row-reverse` |
| Composer 模式 / MCP / palette 等 | 绝对定位，贴边仍可能被挡 |
| `FileContextMenu` 等 | 自有钳制逻辑 |

已确认（对话）：采用**方案 2**——抽共享启发式基础设施，现有菜单逐步迁移；本轮不强制全部 portal。

## 目标

1. 提供统一的几何定位 API（+ 可选 hook），保证浮层落在可读区域内。
2. 裁切边界优先 `.content-pane`，否则视口；边距默认 8px。
3. 贴边时：水平平移（默认偏 trigger 末端右对齐）；竖直方向下方空间不足则上翻。
4. 先收敛 ModelPicker / AgentPicker / SelectMenu，再迁 Composer 高风险菜单；其余触碰再迁。

## 非目标

- 本轮不强制全部浮层改 `createPortal`。
- 不重做浮层视觉样式（毛玻璃、圆角等）。
- 不抽通用 Dropdown/Popover React 组件（只统一几何与定位生命周期）。
- 不解决 z-index / 主题 token 全局重构。

## API 设计

### 文件

- 新建：`frontend/src/lib/clampPopover.ts`
- 测试：`frontend/src/lib/clampPopover.test.ts`
- 废弃/薄包装：`modelPickerFlyout.ts` 改为 re-export 或删并由 ModelPicker 改用新 API

### 类型与函数

```ts
type RectLike = { left: number; top: number; right: number; bottom: number; width: number; height: number }

type Bounds = { left: number; top: number; right: number; bottom: number }

type ClampPopoverInput = {
  anchorRect: Pick<RectLike, "left" | "top" | "right" | "bottom" | "width" | "height">
  popoverSize: { width: number; height: number }
  bounds: Bounds
  pad?: number          // 默认 8
  gap?: number          // 默认 6；浮层与 trigger 间距
  preferAlign?: "start" | "end"  // 默认 "end"（右对齐，适合顶栏）
  placement?: "below" | "above" | "auto"  // 默认 "auto"
}

type ClampPopoverResult = {
  /** 视口坐标（fixed / portal 直接用） */
  left: number
  top: number
  placement: "below" | "above"
  maxHeight: number
  /** 相对 anchor 左上角的偏移（就地 absolute 用） */
  offsetLeft: number
  offsetTop: number
}
```

| 函数 | 职责 |
|------|------|
| `resolveClipBounds(anchor: Element)` | `closest(".content-pane")` 有有效尺寸则用其矩形；否则 `{0,0,innerWidth,innerHeight}` |
| `measurePopoverSize(el: HTMLElement)` | `max(scrollWidth, offsetWidth)` / height 同理，避免 overflow 裁切低估 |
| `clampPopover(input)` | 纯函数：返回完整结果 |
| `useClampPopover({ open, anchorRef, popoverRef, ...opts })` | `useLayoutEffect`：打开时 clamp；监听 resize / scroll(capture)；返回 `CSSProperties`（fixed 用 left/top，或 relative 用 offset） |

### 算法要点

1. **宽度**：`width = min(popoverSize.width, bounds.innerWidth)`（去掉 pad 后）。
2. **水平**：`preferAlign === "end"` 时先按 `anchor.right - width`；溢左/溢右再平移贴边。`"start"` 则先 `anchor.left`。
3. **竖直**：`placement === "auto"` 时，下方可用高 `< min(期望高, 阈值)` 且上方更大则 `above`。
4. **maxHeight**：取所选方向可用空间（减 gap/pad），供菜单内部滚动。
5. **相对偏移**：`offsetLeft = left - anchor.left`，`offsetTop = top - anchor.top`（就地定位时设 `left/top` 并清掉与之冲突的 `right`）。

### Hook 约定

- 测量用 `measurePopoverSize`，不依赖被裁切后的 `getBoundingClientRect().width`。
- 依赖项：`open`、以及调用方传入的 `sizeKey`（如 `editing`、选项数量），避免变宽后不重算。
- 不拥有 portal；是否 portal 由调用方决定。Hook 可通过 `mode: "fixed" | "relative"` 决定返回哪种 style 字段。

## 迁移计划

### 阶段 A — 基础设施（本设计落地第一刀）

1. 实现 `clampPopover` + 单测（水平贴边、content-pane bounds、上下翻转、相对 offset）。
2. `ModelPicker` 改用共享 API；删除或瘦身 `modelPickerFlyout.ts`。
3. `AgentPicker` 改用共享 API（替换内联 VIEWPORT 逻辑）。
4. `SelectMenu.computePos` 改为调用 `clampPopover`（保持 portal + fixed 行为）。

### 阶段 B — Composer 高风险

- `composer-mode-menu`、`composer-mcp-menu`、`composer-palette`：接入 hook 或 portal+clamp，保证贴底/贴侧不被 `chat-pane` / `content-pane` 裁切。

### 阶段 C — 触碰再迁

- `FileContextMenu`、Cron 更多菜单、Providers 内浮层等：逻辑改为共享函数，交互与视觉不变。

每阶段可独立提交；不做大爆炸式一次改完。

## 交互与视觉

- 本设计**不改变**菜单外观；仅改变坐标与可能的 `max-height`。
- ModelPicker 编辑态继续可用 CSS `row-reverse`（编辑在列表内侧），钳制按**整块 flyout**宽高计算。
- z-index 沿用各组件现有值；若 portal 后被挡，个案调高，不在本规格统一 z-index 表。

## 测试

- `clampPopover.test.ts`：node:test，覆盖：
  - 右对齐且放得下 → 右缘贴 trigger 右缘
  - 右侧不够 → 左移且不越 `bounds.right - pad`
  - bounds 为 content-pane 子矩形 → 不越 pane
  - 下方不够、上方够 → `placement === "above"`
  - `offsetLeft/Top` 与绝对坐标一致
- 手动：智能对话顶栏开模型编辑；Agent 切换器贴右；Composer 模式/MCP 贴底。

## 验收

- [x] 共享 API 有单测且通过。
- [x] ModelPicker / AgentPicker / SelectMenu 共用该实现，无重复钳制代码（允许薄包装）。
- [x] 模型编辑展开后整块在 content-pane 内可见（回归）。
- [x] 阶段 B 完成后 Composer 相关菜单贴边不被裁切。
- [x] 阶段 C：FileContextMenu / Cron 更多菜单 / Schedule 日期弹层已迁入。

## 风险

| 风险 | 缓解 |
|------|------|
| relative vs fixed 混用算错偏移 | 测试同时断言绝对坐标与 offset；文档写清 mode |
| scroll 容器非 window | scroll 监听用 capture；bounds 用 content-pane |
| 测量时机（首帧宽高 0） | rAF 二次 clamp；sizeKey 驱动重算 |
