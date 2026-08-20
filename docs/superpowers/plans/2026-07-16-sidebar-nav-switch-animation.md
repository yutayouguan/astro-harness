# Sidebar Navigation Switch Animation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为左侧导航增加一个会在当前菜单项之间平滑移动的共享选中高亮块。

**Architecture:** 在 `App.tsx` 的导航按钮外增加定位容器和唯一高亮元素，React 只把当前菜单索引换算成纵向像素偏移并写入 CSS 自定义属性。`layout.css` 负责高亮块的形状、主题视觉、移动过渡及 reduced-motion 降级；现有按钮继续负责语义、事件、图标与标签。

**Tech Stack:** React 18、TypeScript 5.6、CSS transitions、Vite 5、Tauri 2

## Global Constraints

- 动画时长为 `220ms`，缓动为 `cubic-bezier(0.22, 1, 0.36, 1)`。
- 只动画 `transform`，快速连续点击不得阻塞交互。
- `prefers-reduced-motion: reduce` 时高亮块立即到位。
- 不引入动画依赖，不修改 `AnimatedSwitch`、侧栏展开/收起逻辑或导航行为。
- 首次渲染不播放从顶部滑入的动画。

---

## File Structure

- Modify: `apps/desktop/src/App.tsx` — 计算当前导航索引，渲染共享高亮块并提供纵向偏移。
- Modify: `apps/desktop/src/styles/layout.css` — 统一导航行尺寸，将活动背景迁移到高亮块并定义切换动画。

不创建独立组件：该高亮块只服务当前侧栏，拆分会增加没有复用价值的接口。项目没有 DOM/CSS 单测环境，本次视觉行为通过 TypeScript 构建和手工交互矩阵验证，不为此引入测试框架。

### Task 1: 实现共享导航高亮块

**Files:**
- Modify: `apps/desktop/src/App.tsx:1-9, 93-106, 577-604`
- Modify: `apps/desktop/src/styles/layout.css:1-158`

**Interfaces:**
- Consumes: `nav: NavId`、`NAV` 的固定顺序、现有 `data-tone` 与 `.active` 状态。
- Produces: `.sidebar-nav` 容器、`.sidebar-nav-indicator` 装饰元素、CSS 自定义属性 `--nav-indicator-y`。

- [x] **Step 1: 在 React 中计算高亮位置**

将 `CSSProperties` 加入 React 类型 import：

```tsx
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type CSSProperties,
  type SVGProps,
} from "react";
```

在 `NAV` 后定义与 CSS 导航行尺寸一致的步进，并在 `App` 渲染前计算当前偏移：

```tsx
const NAV_ROW_PITCH_PX = 44;

// App 内，已有 nav state 可用的位置之后
const activeNavIndex = NAV.findIndex((item) => item.id === nav);
const sidebarNavStyle = {
  "--nav-indicator-y": `${Math.max(activeNavIndex, 0) * NAV_ROW_PITCH_PX}px`,
} as CSSProperties;
```

这里使用已计算的像素值，而不是 CSS `calc()` 乘法，保证 Tauri 的 WebView2 和 WKWebView 均可用。

- [x] **Step 2: 渲染导航定位容器和共享高亮块**

用下面结构替换当前直接位于 `<aside>` 下的 `NAV.map(...)`；按钮内部、徽标、tooltip 和 aria 逻辑保持原样：

```tsx
<div className="sidebar-nav" style={sidebarNavStyle}>
  <span className="sidebar-nav-indicator" aria-hidden />
  {NAV.map((item) => {
    const label = t(item.labelKey);
    const pendingBadge =
      item.id === "memory" && chat.memoryPendingCount > 0
        ? chat.memoryPendingCount > 99
          ? "99+"
          : String(chat.memoryPendingCount)
        : null;
    return (
      <button
        key={item.id}
        className={`nav-item ${nav === item.id ? "active" : ""}`}
        data-tone={item.tone}
        onClick={() => setNav(item.id)}
        {...(sidebar.showSidebarLabels
          ? {}
          : { "data-tip": label, "data-tip-pos": "right" as const })}
        aria-label={pendingBadge ? `${label} (${pendingBadge})` : label}
      >
        <span className="nav-icon" aria-hidden>
          <item.Icon />
          {pendingBadge ? <span className="nav-badge">{pendingBadge}</span> : null}
        </span>
        <span className="nav-label">{label}</span>
      </button>
    );
  })}
</div>
```

- [x] **Step 3: 统一导航行布局并定义高亮动画**

在 `layout.css` 的导航样式开头加入容器和高亮块样式。导航行高 `42px`、间距 `2px`，与 `NAV_ROW_PITCH_PX = 44` 对齐：

```css
.sidebar-nav {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 100%;
}

.sidebar-nav-indicator {
  position: absolute;
  z-index: 0;
  top: 0;
  left: 0;
  width: 100%;
  height: 42px;
  border-radius: 14px;
  pointer-events: none;
  transform: translateY(var(--nav-indicator-y, 0));
  transition: transform 220ms cubic-bezier(0.22, 1, 0.36, 1);
  background:
    linear-gradient(155deg, rgba(255, 255, 255, 0.42), rgba(255, 255, 255, 0.12)),
    color-mix(in srgb, var(--tone-soft, transparent) 60%, transparent);
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--tone, transparent) 18%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.45);
  backdrop-filter: blur(12px) saturate(1.15);
  -webkit-backdrop-filter: blur(12px) saturate(1.15);
}

.sidebar.is-icons .sidebar-nav-indicator {
  left: 50%;
  width: 42px;
  border-radius: 12px;
  transform: translate(-50%, var(--nav-indicator-y, 0));
}

html[data-theme="dark"] .sidebar-nav-indicator {
  background:
    linear-gradient(155deg, rgba(255, 255, 255, 0.08), rgba(255, 255, 255, 0.02)),
    color-mix(in srgb, var(--tone-soft) 48%, rgba(8, 6, 18, 0.4));
  box-shadow:
    inset 0 0 0 1px color-mix(in srgb, var(--tone) 26%, transparent),
    inset 0 1px 0 rgba(255, 255, 255, 0.08);
}

@media (prefers-reduced-motion: reduce) {
  .sidebar-nav-indicator {
    transition: none;
  }
}
```

给 `.nav-item` 增加稳定行高和层级：

```css
.nav-item {
  z-index: 1;
  flex: 0 0 42px;
  height: 42px;
  /* 保留其余现有声明 */
}
```

删除 `.nav-item.active` 和 `.nav-item.active[data-tone]` 中的 `background`、`box-shadow`、`backdrop-filter` 声明以及对应暗色覆盖，只保留活动项的文字颜色。保留 `.sidebar.is-labels .nav-item.active::before` 圆点、活动图标填充和所有 tone 变量。

- [x] **Step 4: 构建验证类型和样式入口**

Run:

```bash
cd frontend && npm run build
```

Expected: TypeScript 与 Vite 构建成功，命令退出码为 `0`。

- [x] **Step 5: 手工验证交互矩阵**

Run:

```bash
cd frontend && npm run tauri dev
```

逐项确认：

1. 仅图标模式下，从“智能对话”切到列表首、中、尾部，高亮始终为 `42px × 42px` 并覆盖当前项。
2. 显示文字模式下重复切换，高亮宽度随侧栏展开，左侧活动圆点仍显示。
3. 快速连续点击多个导航项，高亮从当前位置追随最终项，无闪烁且页面仍可点击。
4. 切换明暗主题，各导航 tone 的高亮、边框和阴影与当前主题一致。
5. “记忆空间”存在徽标时，徽标位置与点击区域不变。
6. 系统开启“减少动态效果”后，高亮立即切换且无位移过渡。
7. 应用首次打开时，高亮直接出现在当前项，不从顶部外位置滑入。

Expected: 七项全部通过；关闭开发应用后终端无新增运行时错误。

- [x] **Step 6: 检查并提交实现**

Run:

```bash
git status --short
git diff -- apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git diff --cached
git log -5 --oneline
git add apps/desktop/src/App.tsx apps/desktop/src/styles/layout.css
git commit -m "feat(ui): animate sidebar navigation selection"
git status --short
```

Expected: commit 只包含 `App.tsx` 和 `layout.css`；用户已有的其他未跟踪或未提交文件保持不变。
