# Chat MCP Toggle & Thinking Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 聊天输入栏增加 MCP 快捷开关弹出层；ModelPicker 隐藏并强制关闭 Auto/MAX；仅当模型支持推理时显示推理按钮。

**Architecture:** 新增纯函数 `supportsThinkingControls` + Composer 内独立 `ComposerMcpMenu` 组件复用 `useMcpTools`；`ToolsPanel` 通过一次性 focus 标记切到 MCP tab；`loadPickerGlobals` 静默规范化 `auto/maxMode=false`。

**Tech Stack:** React + TypeScript、现有 `useMcpTools` / `modelPrefs`、`node:test`（与 `autoModelSelect.test.ts` 一致）、CSS 沿用 composer / menu-glass 变量。

**Spec:** `docs/superpowers/specs/2026-07-13-chat-mcp-thinking-controls-design.md`

---

## File map

| File | Responsibility |
|------|----------------|
| Create: `frontend/src/lib/thinkingSupport.ts` | 推理控件显示判定 |
| Create: `frontend/src/lib/thinkingSupport.test.ts` | 判定单测 |
| Create: `frontend/src/components/ComposerMcpMenu.tsx` | MCP 弹出层 UI |
| Modify: `frontend/src/lib/modelPrefs.ts` | `loadPickerGlobals` 强制关闭 auto/max；弱化 `syncMaxModeWithThinkingLevel` |
| Modify: `frontend/src/components/ModelPicker.tsx` | 去掉 Auto/MAX UI；上报当前模型 caps |
| Modify: `frontend/src/components/ChatView.tsx` | 接入 MCP 按钮与菜单 |
| Modify: `frontend/src/components/ToolsPanel.tsx` | 支持一次性打开 MCP tab |
| Modify: `frontend/src/App.tsx` | `showThinkingControls` 新判定；跳转 MCP 设置；发送侧忽略不支持推理的 thinking；去掉 auto 选模死分支 |
| Modify: `frontend/src/i18n/messages.ts` | MCP 菜单中英文案 |
| Modify: `frontend/src/styles/chat.css` | MCP 菜单样式 |

---

### Task 1: `supportsThinkingControls` 纯函数 + 测试

**Files:**
- Create: `frontend/src/lib/thinkingSupport.ts`
- Create: `frontend/src/lib/thinkingSupport.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { supportsThinkingControls } from "./thinkingSupport.ts";

test("reasoning true shows controls", () => {
  assert.equal(
    supportsThinkingControls("openai", { vision: false, web: false, reasoning: true, tools: true }),
    true,
  );
});

test("reasoning false hides even for deepseek", () => {
  assert.equal(
    supportsThinkingControls("deepseek", { vision: false, web: false, reasoning: false, tools: true }),
    false,
  );
});

test("unknown caps falls back to deepseek whitelist", () => {
  assert.equal(supportsThinkingControls("deepseek", null), true);
  assert.equal(supportsThinkingControls("openai", null), false);
  assert.equal(supportsThinkingControls("deepseek", undefined), true);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && node --experimental-strip-types --test src/lib/thinkingSupport.test.ts`

Expected: FAIL (module not found)

- [ ] **Step 3: Write minimal implementation**

```ts
/** 是否在 Composer 显示推理强度控件。 */
import type { ModelCapabilities } from "../types";

const REASONING_BACKEND_FALLBACK = new Set(["deepseek"]);

/**
 * caps 已知时只看 reasoning；
 * caps 未知（null/undefined）时回退 provider backend 白名单。
 */
export function supportsThinkingControls(
  backendId: string | null | undefined,
  caps: ModelCapabilities | null | undefined,
): boolean {
  if (caps != null) return caps.reasoning === true;
  return !!backendId && REASONING_BACKEND_FALLBACK.has(backendId);
}
```

- [ ] **Step 4: Run tests and make sure they pass**

Run: `cd frontend && node --experimental-strip-types --test src/lib/thinkingSupport.test.ts`

Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/thinkingSupport.ts frontend/src/lib/thinkingSupport.test.ts
git commit -m "$(cat <<'EOF'
feat: add thinking controls capability helper

EOF
)"
```

---

### Task 2: 强制关闭 Auto / MAX 偏好

**Files:**
- Modify: `frontend/src/lib/modelPrefs.ts`
- Test: extend with `frontend/src/lib/modelPrefs.globals.test.ts`（仅测规范化逻辑；可用直接调用 `loadPickerGlobals` + mock localStorage）

- [ ] **Step 1: Write failing test for normalize-on-load**

Create `frontend/src/lib/modelPrefs.globals.test.ts`:

```ts
import { test, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { loadPickerGlobals, savePickerGlobals } from "./modelPrefs.ts";

beforeEach(() => {
  // node 环境无 localStorage 时用简易 polyfill
  const store = new Map<string, string>();
  (globalThis as any).localStorage = {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => { store.set(k, v); },
    removeItem: (k: string) => { store.delete(k); },
  };
});

test("loadPickerGlobals forces auto and maxMode off and persists", () => {
  savePickerGlobals({ auto: true, maxMode: true });
  const g = loadPickerGlobals();
  assert.deepEqual(g, { auto: false, maxMode: false });
  assert.deepEqual(loadPickerGlobals(), { auto: false, maxMode: false });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && node --experimental-strip-types --test src/lib/modelPrefs.globals.test.ts`

Expected: FAIL（仍返回 auto/maxMode true）

- [ ] **Step 3: Update `loadPickerGlobals` and neutralize maxMode sync**

Replace `loadPickerGlobals` body so that after parse:

```ts
export function loadPickerGlobals(): ModelPickerGlobals {
  try {
    const raw = localStorage.getItem(GLOBALS_KEY);
    if (!raw) return { ...DEFAULT_PICKER_GLOBALS };
    const parsed = JSON.parse(raw) as Partial<ModelPickerGlobals>;
    const normalized: ModelPickerGlobals = {
      auto: false,
      maxMode: false,
    };
    // 若旧值曾为 true，写回关闭状态
    if (parsed.auto || parsed.maxMode) {
      savePickerGlobals(normalized);
    }
    return normalized;
  } catch {
    return { ...DEFAULT_PICKER_GLOBALS };
  }
}
```

Replace `syncMaxModeWithThinkingLevel` to no longer flip maxMode（思考 max 只走 model prefs effort）：

```ts
/** Auto/MAX UI 已移除：保留 API，避免调用方报错，始终返回规范化 globals */
export function syncMaxModeWithThinkingLevel(_level: ThinkingLevel): ModelPickerGlobals {
  return loadPickerGlobals();
}
```

- [ ] **Step 4: Run tests**

Run: `cd frontend && node --experimental-strip-types --test src/lib/modelPrefs.globals.test.ts src/lib/thinkingSupport.test.ts`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/modelPrefs.ts frontend/src/lib/modelPrefs.globals.test.ts
git commit -m "$(cat <<'EOF'
fix: force-disable model picker auto and max mode

EOF
)"
```

---

### Task 3: ModelPicker 去掉 Auto/MAX UI，并上报当前模型 caps

**Files:**
- Modify: `frontend/src/components/ModelPicker.tsx`

- [ ] **Step 1: Extend Props**

```ts
type Props = {
  // ...existing
  /** 当前选中模型的 capabilities；未知时传 null */
  onActiveModelCapsChange?: (caps: ModelCapabilities | null) => void;
};
```

Import `ModelCapabilities` from `../types`.

- [ ] **Step 2: Remove globals toggles from JSX**

删除 `model-picker-globals` 整块（Auto / MAX ToggleSwitch）以及 `globals.auto` 分支的 auto-hint / trigger「Auto」展示。

触发器始终走 `activeProvider` 模型名分支。

面板始终渲染模型列表（不再 `globals.auto ? hint : list`）。

编辑侧栏条件从 `editing && !globals.auto` 改为 `editing`。

- [ ] **Step 3: Notify caps when options / selection change**

在加载完 `options` / `modelsByProvider` 后，用 `useEffect`：

```ts
useEffect(() => {
  if (!onActiveModelCapsChange) return;
  if (!activeProvider) {
    onActiveModelCapsChange(null);
    return;
  }
  const list = modelsByProvider.get(activeProvider.id); // 按现有 state 名调整
  const info = list?.find((m) => m.id === activeProvider.model);
  onActiveModelCapsChange(info?.capabilities ?? null);
}, [activeProvider, modelsByProvider, onActiveModelCapsChange]);
```

（实现时对照 ModelPicker 现有 state 变量名；若模型列表存在于 `options`/`cache`，用等价结构。）

- [ ] **Step 4: Manual smoke**

Run: `cd frontend && npx tsc -b --pretty false`

Expected: 无 ModelPicker 相关类型错误

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/ModelPicker.tsx
git commit -m "$(cat <<'EOF'
feat: drop Auto/MAX from model picker and report model caps

EOF
)"
```

---

### Task 4: i18n + ComposerMcpMenu 组件

**Files:**
- Modify: `frontend/src/i18n/messages.ts`
- Create: `frontend/src/components/ComposerMcpMenu.tsx`
- Modify: `frontend/src/styles/chat.css`

- [ ] **Step 1: Add i18n keys (zh + en)**

中文（插在 chat.* 附近）：

```ts
"chat.mcpMenu": "MCP 服务",
"chat.mcpMenuSearch": "搜索 MCP 服务…",
"chat.mcpMenuEmpty": "还没有 MCP 服务",
"chat.mcpMenuOpenSettings": "打开 MCP 设置",
"chat.mcpMenuUser": "用户",
```

英文：

```ts
"chat.mcpMenu": "MCP servers",
"chat.mcpMenuSearch": "Search MCP servers…",
"chat.mcpMenuEmpty": "No MCP servers yet",
"chat.mcpMenuOpenSettings": "Open MCP settings",
"chat.mcpMenuUser": "User",
```

- [ ] **Step 2: Create `ComposerMcpMenu.tsx`**

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import { useMcpTools, type McpServer } from "../hooks/useMcpTools";

type Props = {
  open: boolean;
  agentId?: string | null;
  onClose: () => void;
  onOpenSettings: () => void;
  /** 锚定到触发按钮，用于定位（可选；也可用 CSS 相对 composer） */
  anchorRef?: React.RefObject<HTMLElement | null>;
};

export default function ComposerMcpMenu({
  open,
  agentId,
  onClose,
  onOpenSettings,
}: Props) {
  const { t } = useI18n();
  const { servers, toggleServer, ready } = useMcpTools(agentId);
  const [q, setQ] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const filtered = useMemo(() => {
    const needle = q.trim().toLowerCase();
    if (!needle) return servers;
    return servers.filter(
      (s) =>
        s.name.toLowerCase().includes(needle) ||
        s.id.toLowerCase().includes(needle),
    );
  }, [servers, q]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="composer-mcp-menu" ref={rootRef} role="dialog" aria-label={t("chat.mcpMenu")}>
      <input
        className="composer-mcp-search"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={t("chat.mcpMenuSearch")}
        autoFocus
      />
      <div className="composer-mcp-section-label">{t("chat.mcpMenuUser")}</div>
      <ul className="composer-mcp-list">
        {!ready ? null : filtered.length === 0 ? (
          <li className="composer-mcp-empty">{t("chat.mcpMenuEmpty")}</li>
        ) : (
          filtered.map((s: McpServer) => (
            <li key={s.id} className="composer-mcp-row">
              <span className="composer-mcp-name" title={s.name}>{s.name}</span>
              <button
                type="button"
                role="switch"
                aria-checked={s.enabled}
                className={`composer-mcp-switch ${s.enabled ? "is-on" : ""}`}
                onClick={() => toggleServer(s.id)}
              />
            </li>
          ))
        )}
      </ul>
      <button
        type="button"
        className="composer-mcp-settings"
        onClick={() => {
          onOpenSettings();
          onClose();
        }}
      >
        {t("chat.mcpMenuOpenSettings")}
      </button>
    </div>
  );
}
```

- [ ] **Step 3: Add CSS in `chat.css`（靠近 `.composer-mode-menu`）**

使用现有 `--menu-glass-*` 变量，约：

```css
.composer-mcp-menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + 8px);
  width: min(320px, 82vw);
  max-height: min(420px, 60vh);
  display: flex;
  flex-direction: column;
  z-index: 40;
  border-radius: 14px;
  background: var(--menu-glass-bg);
  border: 1px solid var(--menu-glass-border);
  box-shadow: var(--menu-glass-shadow);
  backdrop-filter: var(--menu-glass-blur);
  overflow: hidden;
}
.composer-mcp-search { /* padding, border-bottom, transparent bg */ }
.composer-mcp-list { overflow: auto; flex: 1; margin: 0; padding: 4px 0; list-style: none; }
.composer-mcp-row { display: flex; align-items: center; justify-content: space-between; padding: 8px 12px; gap: 12px; }
.composer-mcp-switch { /* 与 mp-switch / 现有 toggle 尺寸接近的圆角开关 */ }
.composer-mcp-settings { /* 底栏全宽按钮 */ }
```

（实现时对照 `.composer-mode-menu` / `.mp-switch` 抄配色，保证亮暗主题可读。）

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/ComposerMcpMenu.tsx frontend/src/i18n/messages.ts frontend/src/styles/chat.css
git commit -m "$(cat <<'EOF'
feat: add composer MCP servers popup menu

EOF
)"
```

---

### Task 5: ToolsPanel 一次性打开 MCP tab

**Files:**
- Modify: `frontend/src/components/ToolsPanel.tsx`
- Modify: `frontend/src/App.tsx`（仅加 focus helper；ChatView 接线在 Task 6）

- [ ] **Step 1: Add focus helper module inline in ToolsPanel file top**

```ts
const TOOLS_FOCUS_KEY = "astro.tools.focusTab";

export function requestToolsMcpFocus() {
  try {
    sessionStorage.setItem(TOOLS_FOCUS_KEY, "mcp");
  } catch {
    /* ignore */
  }
}

function consumeToolsFocusTab(): ToolTab | null {
  try {
    const v = sessionStorage.getItem(TOOLS_FOCUS_KEY);
    sessionStorage.removeItem(TOOLS_FOCUS_KEY);
    if (v === "mcp" || v === "builtin") return v;
  } catch {
    /* ignore */
  }
  return null;
}
```

- [ ] **Step 2: Consume on active**

在 `ToolsPanel` 内：

```ts
useEffect(() => {
  if (!active) return;
  const focus = consumeToolsFocusTab();
  if (focus) setTab(focus);
}, [active]);
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ToolsPanel.tsx
git commit -m "$(cat <<'EOF'
feat: allow focusing Tools MCP tab via session flag

EOF
)"
```

---

### Task 6: ChatView 接入 MCP 按钮；App 接线判定与发送

**Files:**
- Modify: `frontend/src/components/ChatView.tsx`
- Modify: `frontend/src/App.tsx`

- [ ] **Step 1: ChatView props**

```ts
  onOpenMcpSettings?: () => void;
  /** 当前会话 / workspace agent id，供 MCP 菜单作用域 */
  mcpAgentId?: string | null;
```

- [ ] **Step 2: Composer 状态与按钮**

在 `ChatView` 内：

```ts
const [mcpMenuOpen, setMcpMenuOpen] = useState(false);
const mcpWrapRef = useRef<HTMLDivElement>(null);
```

打开 MCP 时关闭其它 palette / mode menu：

```ts
const openMcpMenu = () => {
  setPaletteKind(null);
  setModeMenuOpen(false);
  setMcpMenuOpen((v) => !v);
};
```

在推理 pill 与 `@` 按钮之间插入：

```tsx
<div className="composer-mcp-wrap" ref={mcpWrapRef}>
  <button
    type="button"
    className={`composer-icon-btn ${mcpMenuOpen ? "is-open" : ""}`}
    title={t("chat.mcpMenu")}
    aria-label={t("chat.mcpMenu")}
    aria-expanded={mcpMenuOpen}
    disabled={false}
    onClick={openMcpMenu}
  >
    {/* 使用 lucide Cable / Plug / Server；项目已用 lucide-react */}
    <Cable size={17} strokeWidth={2} />
  </button>
  <ComposerMcpMenu
    open={mcpMenuOpen}
    agentId={mcpAgentId}
    onClose={() => setMcpMenuOpen(false)}
    onOpenSettings={() => onOpenMcpSettings?.()}
  />
</div>
```

`.composer-mcp-wrap { position: relative; }`

打开 thinking/mention/slash 时 `setMcpMenuOpen(false)`。

- [ ] **Step 3: App state for caps + showThinking**

```ts
const [activeModelCaps, setActiveModelCaps] = useState<ModelCapabilities | null>(null);

// ModelPicker:
onActiveModelCapsChange={setActiveModelCaps}

showThinkingControls={supportsThinkingControls(
  activeProvider?.backend_id,
  activeModelCaps,
)}

onOpenMcpSettings={() => {
  requestToolsMcpFocus();
  setNav("tools");
}}
mcpAgentId={/* 与聊天当前 agent 一致；若 App 已有 currentAgentId / workspace 则传入，否则 "workspace" */}
```

- [ ] **Step 4: 发送侧忽略不支持推理的 thinking；删除 auto 分支**

在 `send` 内：

删除 `if (globals.auto) { ... }` 整块。

替换 modelApi 计算：

```ts
const globals = loadPickerGlobals();
const caps = activeModelCaps; // 或发送前再读；与当前 UI 一致即可
const thinkingOk = supportsThinkingControls(chatProvider.backend_id, caps);
const modelApi = thinkingOk
  ? modelPrefsToApi(loadModelPrefs(chatProvider.id, chatModel), globals)
  : { thinkingEnabled: false, reasoningEffort: "high" as const };
```

注意：原先仅 `deepseek` 才 `modelPrefsToApi`；现改为凡 `thinkingOk` 即传 thinking 参数（与「有推理能力才显示控件」一致）。

- [ ] **Step 5: Typecheck**

Run: `cd frontend && npx tsc -b --pretty false`

Expected: exit 0

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ChatView.tsx frontend/src/App.tsx
git commit -m "$(cat <<'EOF'
feat: wire MCP menu and capability-based thinking controls

EOF
)"
```

---

### Task 7: 手测验收 + 收尾

- [ ] **Step 1: Run unit tests**

Run: `cd frontend && node --experimental-strip-types --test src/lib/thinkingSupport.test.ts src/lib/modelPrefs.globals.test.ts src/lib/autoModelSelect.test.ts`

Expected: all PASS

- [ ] **Step 2: Manual checklist（`npm run tauri dev`）**

1. Composer 有 MCP 按钮；弹出可搜索、开关；底栏进 Tools→MCP
2. ModelPicker 无 Auto/MAX；旧 localStorage 打开后不再显示 Auto
3. deepseek / reasoning=true 模型显示推理 pill；reasoning=false 隐藏
4. 关 MCP 后新对话不再带该服务（与 Tools 面板状态一致）

- [ ] **Step 3: Commit any style/i18n polish if needed**

```bash
git add -u frontend/src
git commit -m "$(cat <<'EOF'
chore: polish chat MCP menu after manual QA

EOF
)"
```

（无改动则跳过）

---

## Spec coverage self-check

| Spec 项 | Task |
|---------|------|
| MCP 弹出开关 + 搜索 | Task 4–6 |
| 打开 MCP 设置 | Task 5–6 |
| 隐藏 Auto/MAX + 强制关闭 | Task 2–3 |
| caps.reasoning 优先 + deepseek 回退 | Task 1, 6 |
| 不支持时发送忽略 thinking | Task 6 |
| i18n | Task 4 |
| 验收清单 | Task 7 |

无 TBD / 占位步骤。
