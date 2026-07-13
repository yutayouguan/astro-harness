# Activity Group + Tool-Call Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 气泡活动列表加父级折叠；子项按 kind 显示图标，展开后明确展示 Input / Output 两行。

**Architecture:** `ChatActivity` 增加 `input`/`output`；`App.tsx` 流式写入分开字段；抽出 `resolveActivityIO` 兼容旧 `detail`；新建 `ActivityGroup` 父折叠；重写 `MsgActivity` 摘要+分区详情。子项始终默认折叠；父级 `defaultOpen = verbosity === "detailed"`。

**Tech Stack:** React 18、TypeScript、Vite、lucide-react、NavIcons、McpIcon、现有 chat.css / i18n、`node:test`

**Spec:** `docs/superpowers/specs/2026-07-13-activity-group-toolcall-polish-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `frontend/src/types.ts` | `ChatActivity` 增加 `input?` / `output?` |
| Create: `frontend/src/lib/resolveActivityIO.ts` | 从 activity 解析 input/output（含旧 detail 兜底） |
| Create: `frontend/src/lib/resolveActivityIO.test.ts` | 上述纯函数测试 |
| Modify: `frontend/src/App.tsx` | 流式写入 `input`/`output` |
| Modify: `frontend/src/i18n/messages.ts` | 父级摘要、Input/Output 文案 |
| Create: `frontend/src/components/ActivityGroup.tsx` | 父折叠容器 |
| Modify: `frontend/src/components/MsgActivity.tsx` | 图标 + Input/Output 分区；子项默认折 |
| Modify: `frontend/src/components/ChatView.tsx` | `ActivityCards` 包 `ActivityGroup` |
| Modify: `frontend/src/styles/chat.css` | 父级/分区样式 |
| Modify: spec 状态 → 已实现 |

**不改：** `MsgReasoning.tsx`、`ChatContextTimeline.tsx`、后端流事件 DTO。

---

### Task 1: `ChatActivity` 字段 + `resolveActivityIO`

**Files:**
- Modify: `frontend/src/types.ts`
- Create: `frontend/src/lib/resolveActivityIO.ts`
- Create: `frontend/src/lib/resolveActivityIO.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { resolveActivityIO } from "./resolveActivityIO.ts";

test("prefers explicit input/output over detail", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "tool",
      title: "x",
      input: "in",
      output: "out",
      detail: "old\n→\nold2",
    }),
    { input: "in", output: "out" },
  );
});

test("splits legacy detail on arrow separator", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "tool",
      title: "x",
      detail: '{"a":1}\n→\nok',
    }),
    { input: '{"a":1}', output: "ok" },
  );
});

test("detail without separator becomes output only", () => {
  assert.deepEqual(
    resolveActivityIO({
      id: "1",
      kind: "memory",
      title: "memory_add",
      detail: "remember this",
    }),
    { output: "remember this" },
  );
});

test("empty activity yields empty io", () => {
  assert.deepEqual(
    resolveActivityIO({ id: "1", kind: "tool", title: "x" }),
    {},
  );
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd frontend && node --test src/lib/resolveActivityIO.test.ts
```

Expected: FAIL (module not found)

- [ ] **Step 3: Extend type + implement helper**

In `frontend/src/types.ts`，把 `ChatActivity` 改为：

```ts
export type ChatActivity = {
  id: string;
  kind: ChatActivityKind;
  title: string;
  detail?: string;
  /** 工具 arguments / 调用入参 */
  input?: string;
  /** 工具 result / 记忆 content */
  output?: string;
  status?: "running" | "done" | "error";
  at?: number;
};
```

Create `frontend/src/lib/resolveActivityIO.ts`：

```ts
import type { ChatActivity } from "../types";

const LEGACY_SEP = "\n→\n";

/** 解析活动卡 Input/Output；优先显式字段，否则兼容旧 detail。 */
export function resolveActivityIO(activity: ChatActivity): {
  input?: string;
  output?: string;
} {
  if (activity.input != null || activity.output != null) {
    return {
      input: activity.input || undefined,
      output: activity.output || undefined,
    };
  }
  const detail = activity.detail;
  if (!detail) return {};
  const i = detail.indexOf(LEGACY_SEP);
  if (i < 0) return { output: detail };
  const input = detail.slice(0, i);
  const output = detail.slice(i + LEGACY_SEP.length);
  return {
    input: input || undefined,
    output: output || undefined,
  };
}

/** 是否有可展开正文 */
export function activityHasBody(activity: ChatActivity): boolean {
  const { input, output } = resolveActivityIO(activity);
  return Boolean(input || output);
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd frontend && node --test src/lib/resolveActivityIO.test.ts
```

Expected: 4 pass

- [ ] **Step 5: Commit**

```bash
git add frontend/src/types.ts frontend/src/lib/resolveActivityIO.ts frontend/src/lib/resolveActivityIO.test.ts
git commit -m "$(cat <<'EOF'
feat(chat): add activity input/output fields and resolver

EOF
)"
```

---

### Task 2: App.tsx 流式写入 `input` / `output`

**Files:**
- Modify: `frontend/src/App.tsx`

- [ ] **Step 1: Update `flushToolDeltas` activity create/update**

在 `flushToolDeltas` 内，新建活动时改为同时写 `input`（保留 `detail` 兼容）：

```ts
activities.push({
  id: actId,
  kind: "tool",
  title: d.name || `tool#${d.index}`,
  input: d.args || undefined,
  detail: d.args || undefined,
  status: "running",
  at: Date.now(),
});
```

更新已有 running 活动时，用 `input` 累积（不再只读 `detail`）：

```ts
const argsSoFar =
  cur.status === "running" ? (cur.input ?? cur.detail ?? "") : "";
const nextArgs = d.args ? argsSoFar + d.args : cur.input ?? cur.detail;
activities[idx] = {
  ...cur,
  id: d.id || cur.id,
  title: d.name || cur.title,
  input: nextArgs || undefined,
  detail: nextArgs || undefined,
  status: "running",
};
```

- [ ] **Step 2: Update `tool_call` activity object**

```ts
const activity: ChatActivity = {
  id,
  kind,
  title: name,
  input: payload.arguments_json || undefined,
  output: payload.result || undefined,
  detail:
    [payload.arguments_json, payload.result]
      .filter(Boolean)
      .join("\n→\n") || undefined,
  status: payload.result ? "done" : "running",
  at: Date.now(),
};
```

- [ ] **Step 3: Update `memory_update` activity object**

```ts
const activity: ChatActivity = {
  id: `mem-${Date.now()}`,
  kind: "memory",
  title: payload.operation || "memory",
  output: payload.content,
  detail: payload.content,
  status: "done",
  at: Date.now(),
};
```

- [ ] **Step 4: Typecheck**

```bash
cd frontend && npx tsc -b --pretty false
```

Expected: exit 0

- [ ] **Step 5: Commit**

```bash
git add frontend/src/App.tsx
git commit -m "$(cat <<'EOF'
feat(chat): write activity input and output from stream events

EOF
)"
```

---

### Task 3: i18n 文案

**Files:**
- Modify: `frontend/src/i18n/messages.ts`

- [ ] **Step 1: Add Chinese keys**（紧挨 `chat.activityCollapse` 之后）

```ts
  "chat.activityGroup": "工具与活动 · {n}",
  "chat.activityGroupExpand": "展开活动列表",
  "chat.activityGroupCollapse": "折叠活动列表",
  "chat.activityInput": "Input",
  "chat.activityOutput": "Output",
```

- [ ] **Step 2: Add English keys**（英文块对应位置）

```ts
  "chat.activityGroup": "Tools & activity · {n}",
  "chat.activityGroupExpand": "Expand activity list",
  "chat.activityGroupCollapse": "Collapse activity list",
  "chat.activityInput": "Input",
  "chat.activityOutput": "Output",
```

（标签用 Input/Output 英文词，符合常见工具调用 UI；如需中文可再改。）

- [ ] **Step 3: Commit**

```bash
git add frontend/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
i18n: add activity group and input/output labels

EOF
)"
```

---

### Task 4: `ActivityGroup` + 重写 `MsgActivity` + 样式 + 接线

**Files:**
- Create: `frontend/src/components/ActivityGroup.tsx`
- Modify: `frontend/src/components/MsgActivity.tsx`
- Modify: `frontend/src/components/ChatView.tsx`
- Modify: `frontend/src/styles/chat.css`

- [ ] **Step 1: Create `ActivityGroup.tsx`**

```tsx
/** 气泡内活动列表父折叠。 */
import { useEffect, useState } from "react";
import { ChevronDown, Wrench } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { ChatActivity } from "../types";
import MsgActivity from "./MsgActivity";

type Props = {
  items: ChatActivity[];
  /** verbosity === "detailed" 时为 true */
  defaultOpen: boolean;
  showTimestamp: boolean;
};

export default function ActivityGroup({
  items,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(defaultOpen);

  useEffect(() => {
    setOpen(defaultOpen);
  }, [defaultOpen]);

  if (!items.length) return null;

  return (
    <div className={`msg-activity-group ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="msg-activity-group-toggle"
        aria-expanded={open}
        aria-label={
          open ? t("chat.activityGroupCollapse") : t("chat.activityGroupExpand")
        }
        onClick={() => setOpen((v) => !v)}
      >
        <Wrench size={14} strokeWidth={2} className="msg-activity-group-icon" aria-hidden />
        <span className="msg-activity-group-label">
          {t("chat.activityGroup", { n: String(items.length) })}
        </span>
        <ChevronDown
          size={14}
          strokeWidth={2}
          className="msg-activity-group-chevron"
          aria-hidden
        />
      </button>
      {open ? (
        <div className="msg-activities">
          {items.map((a) => (
            <MsgActivity
              key={a.id}
              activity={a}
              defaultOpen={false}
              showTimestamp={showTimestamp}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}
```

- [ ] **Step 2: Rewrite `MsgActivity.tsx`**

完整替换为：

```tsx
/** 单条聊天活动卡：kind 图标 + 可折叠 Input/Output。 */
import { useEffect, useState, type ReactNode } from "react";
import { Activity, ChevronDown, Webhook } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import {
  activityHasBody,
  resolveActivityIO,
} from "../lib/resolveActivityIO";
import type { ChatActivity, ChatActivityKind } from "../types";
import McpIcon from "./McpIcon";
import { IconMemory, IconSkills, IconTools } from "./NavIcons";

type Props = {
  activity: ChatActivity;
  defaultOpen: boolean;
  showTimestamp: boolean;
};

function KindIcon({ kind }: { kind: ChatActivityKind }) {
  const common = { "aria-hidden": true as const };
  switch (kind) {
    case "mcp":
      return <McpIcon size={14} {...common} />;
    case "tool":
      return <IconTools width={14} height={14} {...common} />;
    case "skill":
      return <IconSkills width={14} height={14} {...common} />;
    case "memory":
      return <IconMemory width={14} height={14} {...common} />;
    case "hook":
      return <Webhook size={14} strokeWidth={2} {...common} />;
    case "status":
      return <Activity size={14} strokeWidth={2} {...common} />;
  }
}

export default function MsgActivity({
  activity,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const hasBody = activityHasBody(activity);
  const { input, output } = resolveActivityIO(activity);
  const [open, setOpen] = useState(defaultOpen && hasBody);

  useEffect(() => {
    if (hasBody) setOpen(defaultOpen);
  }, [defaultOpen, hasBody]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  const summary: ReactNode = (
    <>
      <span className="msg-activity-kind-icon">
        <KindIcon kind={activity.kind} />
      </span>
      <span className="msg-activity-title">{activity.title}</span>
    </>
  );

  return (
    <div
      className={`msg-activity ${statusClass} ${openClass}`.trim()}
      data-kind={activity.kind}
    >
      <div className="msg-activity-body">
        {hasBody ? (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            aria-label={`${activity.title}，${open ? t("chat.activityCollapse") : t("chat.activityExpand")}`}
            onClick={() => setOpen((v) => !v)}
          >
            {summary}
            <ChevronDown
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        ) : (
          <div className="msg-activity-summary">{summary}</div>
        )}
        {open && (input || output) ? (
          <div className="msg-activity-io">
            {input ? (
              <div className="msg-activity-io-block">
                <span className="msg-activity-io-label">
                  {t("chat.activityInput")}
                </span>
                <pre className="msg-activity-detail">{input}</pre>
              </div>
            ) : null}
            {output ? (
              <div className="msg-activity-io-block">
                <span className="msg-activity-io-label">
                  {t("chat.activityOutput")}
                </span>
                <pre className="msg-activity-detail">{output}</pre>
              </div>
            ) : null}
          </div>
        ) : null}
        {showTimestamp && activity.at ? (
          <span className="msg-activity-time">
            {new Date(activity.at).toLocaleTimeString()}
          </span>
        ) : null}
      </div>
    </div>
  );
}
```

注意：去掉左侧纯文字 `activity.kind` 大写标签，改用图标（spec：按 kind 图标）。若希望保留文字标签，可在图标旁再加 `span.msg-activity-kind`，但默认按本实现（图标优先）。

- [ ] **Step 3: Wire `ActivityCards` in `ChatView.tsx`**

1. 增加 import：

```ts
import ActivityGroup from "./ActivityGroup";
```

2. 可删除 `MsgActivity` 的直接 import（若仅 `ActivityGroup` 使用）。

3. 替换 `ActivityCards` 为：

```tsx
function ActivityCards({
  items,
  prefs,
  showTimestamps,
}: {
  items: ChatActivity[];
  prefs: ChatDisplayPrefs;
  showTimestamps: boolean;
}) {
  const visible = items.filter((a) => isActivityVisible(a.kind, prefs));
  if (!visible.length) return null;
  return (
    <ActivityGroup
      items={visible}
      defaultOpen={prefs.verbosity === "detailed"}
      showTimestamp={showTimestamps}
    />
  );
}
```

- [ ] **Step 4: CSS — 在 `.msg-activities` 之前或附近增加父级样式；调整子项**

在 `chat.css` 的 `.msg-activities` 规则前插入：

```css
.msg-activity-group {
  margin: 0 0 10px;
  border-radius: 12px;
  border: 1px solid color-mix(in srgb, var(--ink) 8%, transparent);
  background: color-mix(in srgb, var(--ink) 2.5%, transparent);
  overflow: hidden;
}

.msg-activity-group-toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  margin: 0;
  padding: 8px 10px;
  border: none;
  background: transparent;
  cursor: pointer;
  text-align: left;
  font: inherit;
  color: var(--ink-mute);
}

.msg-activity-group-toggle:hover {
  color: var(--ink-soft, var(--ink));
}

.msg-activity-group-icon {
  flex-shrink: 0;
  opacity: 0.85;
}

.msg-activity-group-label {
  flex: 1;
  min-width: 0;
  font-size: 12.5px;
  font-weight: 600;
  color: var(--ink);
}

.msg-activity-group-chevron {
  flex-shrink: 0;
  transition: transform 0.15s ease;
}

.msg-activity-group.is-open .msg-activity-group-chevron {
  transform: rotate(180deg);
}

.msg-activity-group .msg-activities {
  margin: 0;
  padding: 0 8px 8px;
  gap: 6px;
}
```

在 `.msg-activity` 相关区域补充（若尚无则加；已有 toggle 保留）：

```css
.msg-activity-kind-icon {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--ink-mute);
}

.msg-activity-summary {
  display: flex;
  align-items: center;
  gap: 6px;
}

.msg-activity-toggle {
  /* 若已存在则确保 gap 含图标 */
  gap: 6px;
}

.msg-activity-io {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 2px;
}

.msg-activity-io-block {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.msg-activity-io-label {
  font-size: 10px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--ink-mute);
}
```

脉冲点仍挂在 `.msg-activity.is-running .msg-activity-title::after`（已有则勿重复）。

子项 `.msg-activity` 在父级内可略减边框对比（可选）：

```css
.msg-activity-group .msg-activity {
  background: color-mix(in srgb, var(--ink) 2%, transparent);
}
```

- [ ] **Step 5: Typecheck + unit tests**

```bash
cd frontend && node --test src/lib/resolveActivityIO.test.ts && npx tsc -b --pretty false
```

Expected: tests pass；tsc exit 0。

若 `IconTools` / `IconSkills` / `IconMemory` 的 props 不含 `aria-hidden`，去掉 spread，仅传 `width`/`height`。

- [ ] **Step 6: Commit**

```bash
git add frontend/src/components/ActivityGroup.tsx frontend/src/components/MsgActivity.tsx frontend/src/components/ChatView.tsx frontend/src/styles/chat.css
git commit -m "$(cat <<'EOF'
feat(chat): parent-fold activity group with input/output rows

EOF
)"
```

---

### Task 5: 手动验收 + 收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-activity-group-toolcall-polish-design.md`

- [ ] **Step 1: Manual checklist**

1. normal：父级默认折；展开后子项默认折；点子项见 Input/Output  
2. detailed：父级默认开；子项仍默认折  
3. kind 图标正确（mcp 为 lobehub MCP）  
4. 流式：先 Input，完成后 Output  
5. 思考块 / 右侧时间线不变  
6. 旧消息仅有 `detail` 仍可展开看到内容  

- [ ] **Step 2: Mark spec implemented**

`**状态:** 已批准` → `**状态:** 已批准 / 已实现`

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-13-activity-group-toolcall-polish-design.md
git commit -m "$(cat <<'EOF'
docs: mark activity group polish spec implemented

EOF
)"
```

---

## Spec Coverage Checklist

| Spec 要求 | Task |
|-----------|------|
| 父折叠 + verbosity 默认 | Task 4 `ActivityGroup` |
| 子项默认折、点开看 IO | Task 4 `MsgActivity` `defaultOpen={false}` |
| Input/Output 分区 | Task 4 |
| kind 图标 | Task 4 `KindIcon` |
| `input`/`output` 字段 | Task 1–2 |
| 旧 detail 兼容 | Task 1 `resolveActivityIO` |
| i18n | Task 3 |
| 不改思考/时间线 | 明确不改文件 |
| 验收 | Task 5 |

## Placeholder / Consistency Self-Review

- 无 TBD；字段名统一 `input`/`output`；父组件名 `ActivityGroup`。
- 子项不再跟随 verbosity 自动展开详情（相对旧 `MsgActivity` 行为的有意变更，符合本 spec）。
