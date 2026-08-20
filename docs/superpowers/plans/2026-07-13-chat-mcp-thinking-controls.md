# Chat MCP + Thinking Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在智能对话 Composer 增加 MCP 快捷开关弹出层，隐藏 ModelPicker 的 Auto/MAX，并仅在支持推理的模型上显示推理控件。

**Architecture:** 纯函数 `shouldShowThinkingControls` 统一推理可见性；`loadPickerGlobals` 强制关闭 Auto/MAX；新组件 `ComposerMcpMenu` 复用 `useMcpTools`；ToolsPanel 接受一次性 `initialTab`；App 接线导航与发送侧 thinking 门控。

**Tech Stack:** React 18、TypeScript、Vite、现有 `useMcpTools` / `modelPrefs` / i18n、`node:test` 单测

**Spec:** `docs/superpowers/specs/2026-07-13-chat-mcp-thinking-controls-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `apps/desktop/src/lib/shouldShowThinkingControls.ts` | 推理按钮可见性判定 |
| Create: `apps/desktop/src/lib/shouldShowThinkingControls.test.ts` | 上述纯函数测试 |
| Modify: `apps/desktop/src/lib/modelPrefs.ts` | 加载 globals 时强制 `auto/maxMode=false` 并写回 |
| Create: `apps/desktop/src/lib/modelPrefsGlobals.test.ts` | globals 规范化测试 |
| Modify: `apps/desktop/src/components/ModelPicker.tsx` | 移除 Auto/MAX UI 与 Auto 触发器文案 |
| Create: `apps/desktop/src/components/ComposerMcpMenu.tsx` | MCP 搜索 + toggle + 打开设置 |
| Modify: `apps/desktop/src/components/ChatView.tsx` | Composer 接入 MCP 按钮/菜单 |
| Modify: `apps/desktop/src/components/ToolsPanel.tsx` | `initialTab` 一次性落到 mcp |
| Modify: `apps/desktop/src/App.tsx` | showThinking、跳转 tools、发送 thinking 门控 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 中英 i18n |
| Modify: `apps/desktop/src/styles/chat.css` | MCP 弹出层样式 |
| Modify: spec 状态 → 已批准 |

---

### Task 1: `shouldShowThinkingControls` 纯函数 + 测试

**Files:**
- Create: `apps/desktop/src/lib/shouldShowThinkingControls.ts`
- Create: `apps/desktop/src/lib/shouldShowThinkingControls.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { shouldShowThinkingControls } from "./shouldShowThinkingControls.ts";

test("uses explicit capabilities.reasoning when present", () => {
  assert.equal(
    shouldShowThinkingControls({
      capabilities: { vision: false, web: false, reasoning: true, tools: true },
      backendId: "openai",
    }),
    true,
  );
  assert.equal(
    shouldShowThinkingControls({
      capabilities: { vision: false, web: false, reasoning: false, tools: true },
      backendId: "deepseek",
    }),
    false,
  );
});

test("falls back to deepseek whitelist when capabilities unknown", () => {
  assert.equal(
    shouldShowThinkingControls({ capabilities: null, backendId: "deepseek" }),
    true,
  );
  assert.equal(
    shouldShowThinkingControls({ capabilities: undefined, backendId: "openai" }),
    false,
  );
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd frontend && node --test src/lib/shouldShowThinkingControls.test.ts`  
Expected: FAIL (module not found)

- [x] **Step 3: Write minimal implementation**

```ts
import type { ModelCapabilities } from "../types";

export function shouldShowThinkingControls(input: {
  capabilities?: ModelCapabilities | null;
  backendId?: string | null;
}): boolean {
  if (input.capabilities != null) {
    return Boolean(input.capabilities.reasoning);
  }
  return input.backendId === "deepseek";
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd frontend && node --test src/lib/shouldShowThinkingControls.test.ts`  
Expected: PASS (2 tests)

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/lib/shouldShowThinkingControls.ts apps/desktop/src/lib/shouldShowThinkingControls.test.ts
git commit -m "$(cat <<'EOF'
feat(chat): add shouldShowThinkingControls helper

EOF
)"
```

---

### Task 2: 强制关闭 Auto / MAX globals

**Files:**
- Modify: `apps/desktop/src/lib/modelPrefs.ts` (`loadPickerGlobals`, optionally no-op `syncMaxModeWithThinkingLevel` max writes)
- Create: `apps/desktop/src/lib/modelPrefsGlobals.test.ts`

- [x] **Step 1: Write the failing test**

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  loadPickerGlobals,
  savePickerGlobals,
} from "./modelPrefs.ts";

test("loadPickerGlobals forces auto and maxMode off and persists", () => {
  const key = "astro.model.pickerGlobals";
  const prev = globalThis.localStorage?.getItem(key) ?? null;
  try {
    // jsdom/node: use a minimal localStorage stub if needed — project tests
    // that touch localStorage should set:
    const store = new Map<string, string>();
    (globalThis as { localStorage: Storage }).localStorage = {
      getItem: (k) => store.get(k) ?? null,
      setItem: (k, v) => { store.set(k, String(v)); },
      removeItem: (k) => { store.delete(k); },
      clear: () => store.clear(),
      key: () => null,
      length: 0,
    };
    savePickerGlobals({ auto: true, maxMode: true });
    const g = loadPickerGlobals();
    assert.equal(g.auto, false);
    assert.equal(g.maxMode, false);
    const raw = store.get(key);
    assert.ok(raw);
    const parsed = JSON.parse(raw!) as { auto: boolean; maxMode: boolean };
    assert.equal(parsed.auto, false);
    assert.equal(parsed.maxMode, false);
  } finally {
    if (prev != null) globalThis.localStorage?.setItem(key, prev);
  }
});
```

- [x] **Step 2: Run test to verify it fails**

Run: `cd frontend && node --test src/lib/modelPrefsGlobals.test.ts`  
Expected: FAIL (`auto` still true)

- [x] **Step 3: Update `loadPickerGlobals`**

Replace body of `loadPickerGlobals` in `apps/desktop/src/lib/modelPrefs.ts` with:

```ts
export function loadPickerGlobals(): ModelPickerGlobals {
  const forced: ModelPickerGlobals = { auto: false, maxMode: false };
  try {
    const raw = localStorage.getItem(GLOBALS_KEY);
    if (!raw) {
      savePickerGlobals(forced);
      return { ...forced };
    }
    const parsed = JSON.parse(raw) as Partial<ModelPickerGlobals>;
    const needsWrite = parsed.auto === true || parsed.maxMode === true || raw.includes("true");
    // Always persist normalized shape so old Auto/MAX users recover.
    if (needsWrite || parsed.auto !== false || parsed.maxMode !== false) {
      savePickerGlobals(forced);
    }
    return { ...forced };
  } catch {
    try {
      savePickerGlobals(forced);
    } catch {
      /* ignore */
    }
    return { ...forced };
  }
}
```

Simpler equivalent (prefer this in implementation):

```ts
export function loadPickerGlobals(): ModelPickerGlobals {
  const forced: ModelPickerGlobals = { auto: false, maxMode: false };
  try {
    const raw = localStorage.getItem(GLOBALS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<ModelPickerGlobals>;
      if (parsed.auto || parsed.maxMode) {
        savePickerGlobals(forced);
      }
    }
  } catch {
    /* ignore */
  }
  return { ...forced };
}
```

Also change `syncMaxModeWithThinkingLevel` to **not** set `maxMode: true` anymore (keep signature, return forced globals):

```ts
export function syncMaxModeWithThinkingLevel(_level: ThinkingLevel): ModelPickerGlobals {
  return loadPickerGlobals();
}
```

- [x] **Step 4: Run test to verify it passes**

Run: `cd frontend && node --test src/lib/modelPrefsGlobals.test.ts`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/lib/modelPrefs.ts apps/desktop/src/lib/modelPrefsGlobals.test.ts
git commit -m "$(cat <<'EOF'
fix(chat): force Auto/MAX picker globals off on load

EOF
)"
```

---

### Task 3: ModelPicker 隐藏 Auto / MAX UI

**Files:**
- Modify: `apps/desktop/src/components/ModelPicker.tsx`

- [x] **Step 1: Remove globals toggles and Auto trigger branch**

In `ModelPicker.tsx`:
1. Delete the entire `<div className="model-picker-globals">…</div>` block (Auto + MAX Mode `ToggleSwitch`).
2. Delete the `{globals.auto ? (…autoHint…) : (…menu…)}` split — always render the model `<ul className="model-picker-menu">…`.
3. In the trigger button, remove `globals.auto ? … : …` branch; always show `activeProvider` / model id (same as current non-auto branch).
4. Change flyout class from `` `model-picker-flyout ${editing && !globals.auto ? "has-edit" : ""}` `` to `` `model-picker-flyout ${editing ? "has-edit" : ""}` ``.
5. Change `{editing && !globals.auto ? (` edit panel to `{editing ? (`.
6. Leave `setGlobal` / globals state if still used by edit panel effort sync; if `globals` becomes unused except load, keep `loadPickerGlobals()` call sites that normalize storage.

- [x] **Step 2: Typecheck**

Run: `cd frontend && npx tsc -b --pretty false 2>&1 | head -40`  
Expected: no errors in ModelPicker

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/components/ModelPicker.tsx
git commit -m "$(cat <<'EOF'
refactor(chat): remove Auto and MAX Mode from ModelPicker UI

EOF
)"
```

---

### Task 4: i18n 文案

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: Add zh + en keys**

Add to Chinese map (near other `chat.*` / `mcpTools.*` keys):

```ts
"chat.mcpMenu": "MCP 服务",
"chat.mcpMenuSearch": "搜索 MCP 服务…",
"chat.mcpMenuEmpty": "还没有配置 MCP 服务",
"chat.mcpMenuOpenSettings": "打开 MCP 设置",
"chat.mcpMenuUserGroup": "用户",
```

Add English counterparts:

```ts
"chat.mcpMenu": "MCP Servers",
"chat.mcpMenuSearch": "Search MCP servers…",
"chat.mcpMenuEmpty": "No MCP servers configured",
"chat.mcpMenuOpenSettings": "Open MCP Settings",
"chat.mcpMenuUserGroup": "User",
```

Ensure `MessageKey` type (if derived from the zh object) still compiles.

- [x] **Step 2: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(i18n): add chat MCP menu strings

EOF
)"
```

---

### Task 5: `ComposerMcpMenu` 组件 + CSS

**Files:**
- Create: `apps/desktop/src/components/ComposerMcpMenu.tsx`
- Modify: `apps/desktop/src/styles/chat.css`

- [x] **Step 1: Create component**

先核对 `AnimatedSwitch` 现有 props（见 `ToolsPanel` 用法），再创建 `ComposerMcpMenu.tsx`：

```tsx
import { useEffect, useMemo, useRef, useState } from "react";
import { PlugZap, Settings2 } from "lucide-react";
import { useMcpTools } from "../hooks/useMcpTools";
import { useI18n } from "../i18n/LocaleContext";
import AnimatedSwitch from "./AnimatedSwitch";

type Props = {
  open: boolean;
  agentId?: string | null;
  onClose: () => void;
  onOpenSettings: () => void;
};

export default function ComposerMcpMenu({
  open,
  agentId,
  onClose,
  onOpenSettings,
}: Props) {
  const { t } = useI18n();
  const { servers, toggleServer } = useMcpTools(agentId);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return servers;
    return servers.filter(
      (s) =>
        s.name.toLowerCase().includes(q) ||
        s.id.toLowerCase().includes(q),
    );
  }, [servers, query]);

  useEffect(() => {
    if (!open) return;
    inputRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onPointer = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("keydown", onKey);
    document.addEventListener("mousedown", onPointer);
    return () => {
      document.removeEventListener("keydown", onKey);
      document.removeEventListener("mousedown", onPointer);
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="composer-mcp-menu" ref={rootRef} role="dialog" aria-label={t("chat.mcpMenu")}>
      <div className="composer-mcp-menu-search">
        <input
          ref={inputRef}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={t("chat.mcpMenuSearch")}
          aria-label={t("chat.mcpMenuSearch")}
        />
      </div>
      <div className="composer-mcp-menu-body">
        {filtered.length === 0 ? (
          <p className="composer-mcp-menu-empty">{t("chat.mcpMenuEmpty")}</p>
        ) : (
          <>
            <div className="composer-mcp-menu-group">{t("chat.mcpMenuUserGroup")}</div>
            <ul className="composer-mcp-menu-list">
              {filtered.map((s) => (
                <li key={s.id} className="composer-mcp-menu-row">
                  <span className="composer-mcp-menu-name" title={s.name}>
                    <PlugZap size={14} strokeWidth={2} aria-hidden />
                    {s.name}
                  </span>
                  {/* AnimatedSwitch: match ToolsPanel prop names exactly */}
                  <AnimatedSwitch
                    checked={s.enabled}
                    onChange={() => toggleServer(s.id)}
                    ariaLabel={s.name}
                  />
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
      <button
        type="button"
        className="composer-mcp-menu-footer"
        onClick={() => {
          onOpenSettings();
          onClose();
        }}
      >
        <Settings2 size={14} strokeWidth={2} aria-hidden />
        {t("chat.mcpMenuOpenSettings")}
      </button>
    </div>
  );
}
```

- [x] **Step 2: Add CSS** near `.composer-palette` in `chat.css`

```css
.composer-mcp-menu {
  position: absolute;
  left: 0;
  bottom: calc(100% + 8px);
  z-index: 40;
  width: min(320px, calc(100vw - 24px));
  max-height: min(420px, 55vh);
  display: flex;
  flex-direction: column;
  border-radius: 14px;
  border: 1px solid var(--menu-glass-border, rgba(255, 255, 255, 0.16));
  background: var(--menu-glass-bg);
  box-shadow: var(--menu-glass-shadow);
  backdrop-filter: var(--menu-glass-blur);
  overflow: hidden;
}
.composer-mcp-menu-search {
  padding: 10px 10px 6px;
}
.composer-mcp-menu-search input {
  width: 100%;
  border-radius: 10px;
  border: 1px solid var(--glass-edge);
  background: var(--glass-inner, rgba(0, 0, 0, 0.2));
  color: var(--ink);
  padding: 8px 10px;
  font: inherit;
}
.composer-mcp-menu-body {
  overflow: auto;
  padding: 4px 6px 8px;
  flex: 1;
}
.composer-mcp-menu-group {
  font-size: 11px;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--ink-mute);
  padding: 6px 8px 4px;
}
.composer-mcp-menu-list {
  list-style: none;
  margin: 0;
  padding: 0;
}
.composer-mcp-menu-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 8px;
  border-radius: 10px;
}
.composer-mcp-menu-row:hover {
  background: color-mix(in srgb, var(--ink) 6%, transparent);
}
.composer-mcp-menu-name {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--ink);
  font-size: 13px;
}
.composer-mcp-menu-empty {
  margin: 12px 8px;
  color: var(--ink-mute);
  font-size: 13px;
}
.composer-mcp-menu-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: 0;
  border-top: 1px solid var(--glass-edge);
  background: transparent;
  color: var(--ink-soft);
  padding: 10px 12px;
  font: inherit;
  cursor: pointer;
  text-align: left;
}
.composer-mcp-menu-footer:hover {
  color: var(--ink);
  background: color-mix(in srgb, var(--ink) 5%, transparent);
}
.composer-mcp-wrap {
  position: relative;
}
.composer-icon-btn.has-dot::after {
  content: "";
  position: absolute;
  top: 5px;
  right: 5px;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: var(--tone-green, #4ade80);
}
```

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/components/ComposerMcpMenu.tsx apps/desktop/src/styles/chat.css
git commit -m "$(cat <<'EOF'
feat(chat): add ComposerMcpMenu popup

EOF
)"
```

---

### Task 6: ChatView 接入 MCP 按钮

**Files:**
- Modify: `apps/desktop/src/components/ChatView.tsx`

- [x] **Step 1: Extend props**

Add to ChatView props:

```ts
agentId?: string | null;
onOpenMcpSettings?: () => void;
```

- [x] **Step 2: Wire button + menu in composer-bar-left**

After thinking pill block, before `@` button:

```tsx
const [mcpOpen, setMcpOpen] = useState(false);
// close mcp when opening other palettes / mode menu
```

When opening thinking / mention / slash / mode menu, set `mcpOpen` false. When opening mcp, set `paletteKind` null and `modeMenuOpen` false.

```tsx
<div className="composer-mcp-wrap">
  <button
    type="button"
    className={`composer-icon-btn ${mcpOpen ? "is-open" : ""} ${/* has enabled */ ""}`}
    disabled={streaming}
    title={t("chat.mcpMenu")}
    aria-label={t("chat.mcpMenu")}
    aria-expanded={mcpOpen}
    onClick={() => {
      setModeMenuOpen(false);
      setPaletteKind(null);
      setMcpOpen((v) => !v);
    }}
  >
    <PlugZap size={16} strokeWidth={2} />
  </button>
  <ComposerMcpMenu
    open={mcpOpen}
    agentId={agentId}
    onClose={() => setMcpOpen(false)}
    onOpenSettings={() => onOpenMcpSettings?.()}
  />
</div>
```

For the green dot: either lift a tiny `useMcpTools(agentId)` in ChatView only for `servers.some(s => s.enabled)`, or pass `mcpEnabledCount` from App. Prefer calling `useMcpTools` once in ChatView and pass servers into menu **or** keep menu owning the hook (two hook instances share persistence via backend — OK for this app). Spec: 小圆点 when any enabled — add `has-dot` class when `useMcpTools` reports any enabled.

Import `PlugZap` from `lucide-react`.

- [x] **Step 3: Typecheck**

Run: `cd frontend && npx tsc -b --pretty false 2>&1 | head -50`  
Expected: only missing App props until Task 8, or fix ChatView optional props so tsc passes.

- [x] **Step 4: Commit**

```bash
git add apps/desktop/src/components/ChatView.tsx
git commit -m "$(cat <<'EOF'
feat(chat): wire MCP menu button in composer

EOF
)"
```

---

### Task 7: ToolsPanel `initialTab`

**Files:**
- Modify: `apps/desktop/src/components/ToolsPanel.tsx`
- Modify: `apps/desktop/src/App.tsx` (minimal: state + pass prop; full ChatView wiring in Task 8)

- [x] **Step 1: Extend ToolsPanel props**

```ts
type Props = {
  active?: boolean;
  /** 打开时落到该 tab；消费后通知父级清空 */
  initialTab?: ToolTab | null;
  onInitialTabConsumed?: () => void;
};
```

Export `ToolTab` if needed, or keep internal and type App as `"builtin" | "mcp"`.

```ts
export default function ToolsPanel({
  active = true,
  initialTab = null,
  onInitialTabConsumed,
}: Props) {
  const [tab, setTab] = useState<ToolTab>("builtin");

  useEffect(() => {
    if (!initialTab) return;
    setTab(initialTab);
    onInitialTabConsumed?.();
  }, [initialTab, onInitialTabConsumed]);
  // ...
}
```

- [x] **Step 2: In App, add state**

```ts
const [toolsInitialTab, setToolsInitialTab] = useState<"builtin" | "mcp" | null>(null);
```

```tsx
{nav === "tools" && (
  <ToolsPanel
    active={nav === "tools"}
    initialTab={toolsInitialTab}
    onInitialTabConsumed={() => setToolsInitialTab(null)}
  />
)}
```

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src/components/ToolsPanel.tsx apps/desktop/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(tools): support initialTab for MCP deep-link

EOF
)"
```

---

### Task 8: App 接线 — showThinking、MCP 跳转、发送门控

**Files:**
- Modify: `apps/desktop/src/App.tsx`

- [x] **Step 1: Resolve capabilities for active model**

Near `activeProvider`:

```ts
import { shouldShowThinkingControls } from "./lib/shouldShowThinkingControls";
import { inferModelCapabilities } from "./lib/modelCaps";

// Prefer explicit model list entry if App already caches models; else null.
// If providers carry no ModelInfo list in App state, pass capabilities: null
// so deepseek whitelist applies. Optional enhancement: look up from a
// models-by-provider cache if one exists in App.
const activeModelCaps = null as import("./types").ModelCapabilities | null;
// If ModelPicker/Providers already expose listed models on provider objects,
// resolve here. Otherwise leave null.

const showThinking = shouldShowThinkingControls({
  capabilities: activeModelCaps,
  backendId: activeProvider?.backend_id,
});
```

**增强（推荐一并做）：** 若 App / providers 状态里能拿到当前 `model` 的 `ModelInfo`，传入其 `capabilities`；否则：

```ts
const showThinking = shouldShowThinkingControls({
  capabilities: activeProvider
    ? inferModelCapabilities(activeProvider.model, activeProvider.kind)
    : null,
  backendId: activeProvider?.backend_id,
});
```

注意：用 `inferModelCapabilities` 时 caps **不再是 unknown**，deepseek-chat 可能 `reasoning:false`。为贴合 spec「未知才回退」：

```ts
function resolveActiveCapabilities(
  provider: ProviderConfig | undefined,
  listed: ModelInfo | undefined,
): ModelCapabilities | null {
  if (listed?.capabilities) return listed.capabilities;
  return null; // unknown → deepseek fallback
}
```

实现时：有列表命中用列表；否则 `null`（不要用 infer 填满，以免吃掉 deepseek 回退）。

- [x] **Step 2: Pass props to ChatView**

```tsx
showThinkingControls={showThinking}
agentId={/* active agent id already used elsewhere, e.g. normalizeAgentId */}
onOpenMcpSettings={() => {
  setToolsInitialTab("mcp");
  setNav("tools");
}}
```

查找 App 中现有 `activeAgentId` / session agent；若没有，传 `null`（`useMcpTools` 默认 workspace 作用域）。

- [x] **Step 3: Gate send-path thinking**

Replace:

```ts
const modelApi =
  chatProvider.backend_id === "deepseek"
    ? modelPrefsToApi(loadModelPrefs(chatProvider.id, chatModel), globals)
    : { thinkingEnabled: false, reasoningEffort: "high" as const };
```

With:

```ts
const sendSupportsThinking = shouldShowThinkingControls({
  capabilities: /* same resolve for chatProvider+chatModel */,
  backendId: chatProvider.backend_id,
});
const modelApi = sendSupportsThinking
  ? modelPrefsToApi(loadModelPrefs(chatProvider.id, chatModel), loadPickerGlobals())
  : { thinkingEnabled: false, reasoningEffort: "high" as const };
```

若 UI thinking prefs 与 modelPrefs 双轨：保持现有 `thinkingPrefs` → API 映射路径，但仅当 `sendSupportsThinking` 为真时启用。

- [x] **Step 4: Typecheck + unit tests**

Run:

```bash
cd frontend && node --test src/lib/shouldShowThinkingControls.test.ts src/lib/modelPrefsGlobals.test.ts
cd frontend && npx tsc -b --pretty false
```

Expected: all PASS / no tsc errors

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(chat): gate thinking UI/send and deep-link MCP settings

EOF
)"
```

---

### Task 9: Spec 状态 + 手工验收清单

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-chat-mcp-thinking-controls-design.md`

- [x] **Step 1: Update status line to `已批准 / 已实现计划`**

- [x] **Step 2: Manual smoke (dev)**

Run: `cd frontend && npm run tauri dev`（或 `npm run dev` + 已有壳）

Checklist:
1. Composer 出现 MCP 按钮；弹出可搜索、开关；底栏进 Tools→MCP
2. ModelPicker 无 Auto/MAX；旧 localStorage 被清掉后显示具体模型
3. deepseek 或 caps.reasoning 模型显示推理 pill；普通模型隐藏
4. 关 MCP 后下一轮对话不再带该服务（与 Tools 页一致）

- [x] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-13-chat-mcp-thinking-controls-design.md
git commit -m "$(cat <<'EOF'
docs: mark chat MCP/thinking controls spec approved

EOF
)"
```

---

## Spec coverage self-check

| Spec item | Task |
|-----------|------|
| MCP 弹出开关 + 搜索 + 空态 | 5, 6 |
| 打开 MCP 设置 → tools/mcp | 7, 8 |
| 互斥关闭 / Esc / 外点 | 5, 6 |
| 隐藏 Auto/MAX UI | 3 |
| 加载强制关闭并写回 | 2 |
| showThinking caps \|\| deepseek | 1, 8 |
| 发送忽略不支持推理 | 8 |
| i18n | 4 |
| 验收项 | 9 |

无 TBD 占位；类型名 `shouldShowThinkingControls` / `ComposerMcpMenu` / `initialTab` 前后一致。
