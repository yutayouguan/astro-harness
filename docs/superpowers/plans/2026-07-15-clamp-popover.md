# clampPopover Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 共享启发式浮层定位，ModelPicker / AgentPicker / SelectMenu 共用，避免被 content-pane 裁切。

**Architecture:** 纯函数 `clampPopover` + `resolveClipBounds` / `measurePopoverSize`；可选 `useClampPopover` hook。阶段 A 收敛三处现有钳制；阶段 B 再迁 Composer。

**Tech Stack:** TypeScript、React、node:test

## Global Constraints

- 不强制全部 portal；不改浮层视觉；不抽通用 Dropdown 组件
- 默认 pad=8、gap=6；bounds 优先 `.content-pane`
- 单测用 `node --experimental-strip-types --test`

---

### Task 1: clampPopover 纯函数 + 测试

**Files:**
- Create: `frontend/src/lib/clampPopover.ts`
- Create: `frontend/src/lib/clampPopover.test.ts`
- Delete or thin: `frontend/src/lib/modelPickerFlyout.ts` / `.test.ts`（Task 2）

**Produces:** `clampPopover`, `resolveClipBounds`, `measurePopoverSize` 及导出类型

- [x] 写失败测试（右对齐、右溢左移、pane bounds、上翻、offset）
- [x] 实现最小通过代码
- [x] `node --experimental-strip-types --test src/lib/clampPopover.test.ts`
- [x] Commit

### Task 2: ModelPicker 迁移

**Files:**
- Modify: `frontend/src/components/ModelPicker.tsx`
- Delete: `frontend/src/lib/modelPickerFlyout.ts`, `modelPickerFlyout.test.ts`

- [x] 改用 `clampPopover` + `resolveClipBounds` + `measurePopoverSize`
- [x] 相对定位 style：`left: offsetLeft, right: "auto"`（top 可仍用 CSS）
- [x] 跑 clamp 测试；手动回归编辑面板
- [x] Commit

### Task 3: AgentPicker 迁移

**Files:**
- Modify: `frontend/src/components/AgentPicker.tsx`

- [x] 替换内联 VIEWPORT 钳制为共享 API（`preferAlign: "start"` 或按现有左/右逻辑）
- [x] Commit

### Task 4: SelectMenu 迁移

**Files:**
- Modify: `frontend/src/components/SelectMenu.tsx`

- [x] `computePos` 改为调用 `clampPopover`（fixed / portal，`preferAlign: "start"`）
- [x] 保持 openUp / maxHeight 行为
- [x] Commit

### Task 5（可选本轮）: useClampPopover + Composer

**Files:**
- Create: `frontend/src/hooks/useClampPopover.ts`
- Modify: Composer 相关（`ChatView.tsx` 模式/MCP/palette）

- [x] hook 已建
- [x] Composer 模式 / MCP / 上下文用量：portal + `useClampPopover`（`placement: above`）
- [x] Palette 仍为输入区上方文档流布局（自带 max-height），本轮不改

### Task 6: 阶段 C — 触碰迁移

**Files:**
- Modify: `FileContextMenu.tsx`, `CronPanel.tsx`, `ScheduleEditor.tsx`
- Modify: `clampPopover.ts`（`pointAnchor` / `resolveClipBoundsAt`）

- [x] FileContextMenu 改用共享 API（光标点锚）
- [x] Cron 更多菜单 / Schedule 日期弹层改用共享 API
- [x] 测试 + Commit

### Task 7: 四向 tip 扩展

**Files:**
- Modify: `clampPopover.ts`（`clampFloatingTip`）
- Modify: `useBeautifyTips.ts`, `ChatMessageNav.tsx`, `chat.css`

- [x] `clampFloatingTip` + 单测（顶/左翻侧、水平钳位与箭头）
- [x] `useBeautifyTips` 改用共享 tip 钳制（含 content-pane bounds）
- [x] ChatMessageNav 预览标签改 left/top + 可 flip 右侧
- [x] Commit

### Task 8: 随访收口

**Files:**
- Create: `frontend/src/hooks/useAnchoredMenu.ts`
- Modify: `AgentPicker.tsx`, `SelectMenu.tsx`, `CronPanel.tsx`, `ModelPicker.tsx`, `ChatMessageNav.tsx`
- Modify: `base.css`（`--z-drawer` / `--z-menu` / `--z-tip`）及菜单/抽屉/tip 样式

- [x] `useAnchoredMenu`：portal 菜单测量 + rAF + clamp；迁 AgentPicker / SelectMenu / Cron
- [x] ModelPicker 改用 `useClampPopover`（relative，仅水平）
- [x] ChatMessageNav：估宽首帧 + 量完精调
- [x] CSS 叠放 token；Skills/MCP 全屏抽屉与 Providers 拖拽 ghost 不迁 clamp
- [x] 测试 + Commit

### Task 9: 随访收口（二）

**Files:**
- Create: `frontend/src/lib/anchoredMenuLayout.ts` + `.test.ts`
- Modify: `ScheduleEditor.tsx`, `SelectMenu.tsx`, `AgentPicker.tsx`, `CronPanel.tsx`

- [x] ScheduleEditor 日期弹层改用 `useAnchoredMenu`
- [x] SelectMenu `maxWidth` 用 `pos.widthCap`（content-pane bounds）
- [x] AgentPicker / Cron 更多菜单：方向键、Escape 回焦点
- [x] `layoutAnchoredMenu` 单测
- [x] Commit

---

## 验证

```bash
cd frontend && node --experimental-strip-types --test src/lib/clampPopover.test.ts src/lib/anchoredMenuLayout.test.ts
npx tsc -b --pretty false
```
