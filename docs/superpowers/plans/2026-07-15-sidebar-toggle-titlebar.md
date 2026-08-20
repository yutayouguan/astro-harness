# Sidebar Toggle Titlebar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 把「隐藏/固定侧栏」按钮从侧栏品牌区挪到标题栏红绿灯右侧，全局常驻可点。

**Architecture:** 在 `app-shell` 内、`native-drag-region` 旁挂常驻 `.titlebar-sidebar-toggle`；点击仍走现有 `toggleSidebar()`。品牌区只留标签开关。不抽新组件，不改 pin 语义。

**Tech Stack:** React (`App.tsx`) + 现有 shell CSS / NavIcons / i18n。

**参考:** [设计规格](../specs/2026-07-15-sidebar-toggle-titlebar-design.md)

## Global Constraints

- 只挪 pin/unpin；标签开关留在 `sidebar-brand-actions`。
- 交互继续调用 `toggleSidebar()`；不改 `astro.sidebarPinned` / 悬停热区。
- 复用 `IconPanelOpen` / `IconPanelClose` 与 `sidebar.pin*` / `sidebar.unpin*`。
- 不做快捷键；不重做侧栏动画/宽度。
- 本改动无纯逻辑可测点：以手动验收为准（不为此挂装全量 App RTL）。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `apps/desktop/src/styles/shell.css` | `.titlebar-sidebar-toggle` 定位与按钮外观；品牌区仅一按钮时布局保持可用 |
| Modify: `apps/desktop/src/App.tsx` | 标题栏挂 pin 按钮；从 `sidebar-brand-actions` 删除 pin 按钮 |

---

### Task 1: 标题栏侧栏开关样式

**Files:**
- Modify: `apps/desktop/src/styles/shell.css`（紧接 `.native-drag-region` 之后）

**Interfaces:**
- Consumes: `--titlebar-h`、`--titlebar-traffic-w`（`base.css` 已有）
- Produces: 类名 `.titlebar-sidebar-toggle`（绝对定位容器 + 内部复用或镜像 `.sidebar-pin-btn` 外观）

- [x] **Step 1: 在 `shell.css` 加入标题栏开关样式**

紧接 `.native-drag-region { ... }` 块之后插入：

```css
/* 红绿灯右侧：常驻侧栏固定/隐藏开关（浮在 drag region 之上） */
.titlebar-sidebar-toggle {
  position: absolute;
  top: 0;
  left: var(--titlebar-traffic-w);
  z-index: 45;
  height: var(--titlebar-h);
  display: flex;
  align-items: center;
  padding-left: 2px;
  -webkit-app-region: no-drag;
  app-region: no-drag;
}

.titlebar-sidebar-toggle .sidebar-pin-btn {
  /* 继承既有 .sidebar-pin-btn；标题栏处略收紧避免撞红绿灯 */
  width: 28px;
  height: 28px;
}
```

说明：按钮本身继续用 `className="sidebar-pin-btn"`，避免复制一整套 tone 变体。

- [x] **Step 2: 目测样式不破坏现有侧栏品牌按钮**

在 `npm run tauri dev` 下确认：侧栏内标签开关仍是 28×28；标题栏区域尚无按钮也没关系（Task 2 再挂 markup）。

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/styles/shell.css
git commit -m "$(cat <<'EOF'
style(shell): add titlebar sidebar toggle positioning

EOF
)"
```

---

### Task 2: 挪按钮 markup + 验收

**Files:**
- Modify: `apps/desktop/src/App.tsx`（`return` 开头 shell 区域与 `sidebar-brand-actions`）

**Interfaces:**
- Consumes: `toggleSidebar`、`sidebarPinned`、`activeTone`、`t`、`IconPanelOpen` / `IconPanelClose`；类名 `.titlebar-sidebar-toggle`
- Produces: 标题栏常驻 pin 按钮；品牌区仅标签开关

- [x] **Step 1: 在 `native-drag-region` 之后插入标题栏按钮**

在：

```tsx
      <div
        className="native-drag-region"
        onMouseDown={(e) => void onTitleMouseDown(e)}
        onDoubleClick={(e) => void onTitleDoubleClick(e)}
        aria-hidden
      />
```

之后、`sidebar-hotzone` 之前，插入：

```tsx
      <div className="titlebar-sidebar-toggle">
        <button
          type="button"
          className="sidebar-pin-btn"
          data-tone={activeTone}
          onClick={toggleSidebar}
          title={sidebarPinned ? t("sidebar.unpin") : t("sidebar.pin")}
          aria-label={
            sidebarPinned ? t("sidebar.unpinAria") : t("sidebar.pinAria")
          }
          aria-pressed={sidebarPinned}
        >
          {sidebarPinned ? (
            <IconPanelClose width={15} height={15} />
          ) : (
            <IconPanelOpen width={15} height={15} />
          )}
        </button>
      </div>
```

- [x] **Step 2: 从品牌区删除 pin/unpin 按钮**

`sidebar-brand-actions` 内只保留标签开关那一个 `<button>`；删除原先 `onClick={toggleSidebar}` 的第二个 button（含 `IconPanelClose` / `IconPanelOpen`）。

结果应类似：

```tsx
            <div className="sidebar-brand-actions">
              <button
                type="button"
                className="sidebar-pin-btn"
                data-tone={activeTone}
                onClick={toggleSidebarLabels}
                title={sidebarLabels ? t("sidebar.hideLabels") : t("sidebar.showLabels")}
                aria-label={
                  sidebarLabels ? t("sidebar.hideLabelsAria") : t("sidebar.showLabelsAria")
                }
                aria-pressed={sidebarLabels}
              >
                {sidebarLabels ? (
                  <IconSidebarIcons width={15} height={15} />
                ) : (
                  <IconSidebarLabels width={15} height={15} />
                )}
              </button>
            </div>
```

- [x] **Step 3: 手动验收（对照规格验收条）**

在 `cd frontend && npm run tauri dev`：

1. 侧栏固定：红绿灯旁有收起图标；品牌区只有标签开关。
2. 点收起：侧栏隐藏，标题栏按钮仍在，图标变为展开。
3. 侧栏隐藏时点标题栏按钮：侧栏固定打开。
4. 未固定时左缘热区仍可悬停展开；拖标题栏空白处可拖窗，点按钮不误拖。
5. 切换到记忆 / 工作空间等导航：标题栏按钮仍可用。

Expected: 5 条全部通过。

- [x] **Step 4: Commit**

```bash
git add apps/desktop/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(shell): move sidebar pin toggle next to traffic lights

EOF
)"
```

---

## Spec coverage (self-review)

| Spec 要求 | Task |
|-----------|------|
| 标题栏红绿灯旁常驻 pin 按钮 | Task 1 + 2 |
| 品牌区只留标签开关 | Task 2 Step 2 |
| `toggleSidebar` / localStorage / 图标 / i18n | Task 2 Step 1（复用，不改逻辑） |
| 保留左缘热区 | 未触碰 `sidebar-hotzone` |
| 非 chat-header、全导航可见 | Task 2 挂在 `app-shell` |
| 验收 1–5 | Task 2 Step 3 |
