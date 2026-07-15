# 侧栏开关移至标题栏（红绿灯旁）

日期：2026-07-15  
状态：设计已确认，待实现计划

## 背景

隐藏/固定侧栏按钮目前放在 `sidebar-brand` 右侧（与 logo、标题、「图标/文字」开关并列）。收起侧栏后，按钮随侧栏消失，用户主要依赖左缘热区悬停展开，可达性差，且品牌区偏挤。

参考 Cursor 等 macOS 应用：侧栏开关常驻在红绿灯右侧标题栏，收起后仍可一键展开。

## 目标

1. 将「隐藏/固定侧栏」按钮移到标题栏、红绿灯右侧，全局常驻。
2. 侧栏品牌区只保留「图标/文字标签」开关。
3. 保持现有 pin / 悬停临时展开语义与 localStorage 偏好不变。

## 非目标

- 不改标签开关逻辑或位置。
- 不改 `sidebarPinned` / 悬停临时展开 / 左缘热区行为。
- 不重做侧栏宽度、动画、视觉主题。
- 本轮不加快捷键。
- 不改 pin/unpin 相关 i18n 语义（可复用现有 key）。

## 决策

| 项 | 选择 |
|----|------|
| 范围 | 只挪 pin/unpin；标签开关留在品牌区 |
| 放置 | 标题栏常驻（方案 1），非「仅收起时出现」或双入口 |
| 交互 | 继续调用现有 `toggleSidebar()` |
| 热区 | 保留左缘悬停热区作为补充 |

## 布局

- 在 `native-drag-region` 之上新增常驻 `sidebar-toggle` 控件，水平位置在红绿灯之后（`padding-left: var(--titlebar-traffic-w)` 对齐），垂直居中于 `--titlebar-h`（52px）。
- `z-index` 高于拖拽层（约 45，与既有 header 可点控件一致），`app-region: no-drag` / `-webkit-app-region: no-drag`。
- 尺寸与视觉对齐现有 `sidebar-pin-btn`（约 15px 图标 + 小圆角按钮）；可按需抽共享 class 或在 `shell.css` 增加标题栏专用样式。
- 从 `sidebar-brand-actions` 移除 pin/unpin 按钮；品牌区仅保留标签开关。标签-only 时微调 `sidebar-brand` 布局，避免空位或错位。
- 该按钮属于 app shell，不挂在 chat header 上，所有导航页可见。

## 交互与状态

- 点击：`toggleSidebar()`——切换 `sidebarPinned`，同步 `sidebarOpen`，写入 `astro.sidebarPinned`；清除待执行的 hide timer。
- **已固定**：`IconPanelClose`，title/aria 用 `sidebar.unpin` / `sidebar.unpinAria`。
- **未固定**：`IconPanelOpen`，title/aria 用 `sidebar.pin` / `sidebar.pinAria`。
- `aria-pressed` 绑定 `sidebarPinned`。
- 未固定时：左缘热区与悬停临时展开逻辑不变。

## 实现触及点

- `frontend/src/App.tsx`：按钮 DOM 从品牌区挪到标题栏区域。
- `frontend/src/styles/shell.css`（必要时 `header.css`）：标题栏按钮定位与品牌区只留标签开关时的布局。
- 复用现有图标组件与 i18n key；无需改文案语义。

## 验收

1. 侧栏固定时：标题栏红绿灯旁有收起按钮；品牌区只有标签开关。
2. 点击收起：侧栏隐藏；标题栏按钮仍在，图标/文案变为「固定/展开」。
3. 侧栏隐藏时点击标题栏按钮：侧栏固定打开。
4. 未固定时左缘热区悬停仍可临时展开；拖窗口标题栏区域不被按钮抢拖拽（按钮本身可点）。
5. 各主导航页（对话 / 记忆 / 工作空间等）标题栏按钮均可用。
