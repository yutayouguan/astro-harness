# Activity Cards Collapse Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 气泡内活动卡（工具/记忆等）详情默认折叠，用户可点击展开；`detailed` 详细度下默认展开。

**Architecture:** 抽出 `MsgActivity`（对齐 `MsgReasoning`），本地 `open` 状态；`defaultOpen` 由 `verbosity === "detailed"` 驱动并在偏好变化时同步。`ActivityCards` 只负责可见性过滤与映射。

**Tech Stack:** React 18、TypeScript、Vite、lucide-react、现有 `chat.css` / i18n

**Spec:** `docs/superpowers/specs/2026-07-13-activity-cards-collapse-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `apps/desktop/src/components/MsgActivity.tsx` | 单条活动卡：摘要行 + 可折叠 detail |
| Modify: `apps/desktop/src/components/ChatView.tsx` | `ActivityCards` 改用 `MsgActivity`；移除内联 detail 渲染 |
| Modify: `apps/desktop/src/styles/chat.css` | 活动卡 toggle / chevron / `is-open` 样式 |
| Modify: `apps/desktop/src/i18n/messages.ts` | 展开/折叠详情的 `aria-label` 中英文案 |
| Modify: `docs/superpowers/specs/2026-07-13-activity-cards-collapse-design.md` | 状态改为已实现（收尾） |

**不改：** `MsgReasoning.tsx`、`ChatContextTimeline.tsx`、后端流事件、`useChatDisplayPrefs` 可见性逻辑。

---

### Task 1: i18n 文案

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: 在中文与英文区块各加两条 key**

在中文 `chat` 相关键附近（约 `chat.thinkingDoneWithTime` 之后）加入：

```ts
  "chat.activityExpand": "展开详情",
  "chat.activityCollapse": "折叠详情",
```

在英文对应位置加入：

```ts
  "chat.activityExpand": "Expand details",
  "chat.activityCollapse": "Collapse details",
```

- [x] **Step 2: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
i18n: add activity card expand/collapse labels

EOF
)"
```

---

### Task 2: 新建 `MsgActivity` 组件

**Files:**
- Create: `apps/desktop/src/components/MsgActivity.tsx`

- [x] **Step 1: 创建组件文件**

```tsx
/** 单条聊天活动卡：摘要行 + 可折叠详情。 */
import { useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { ChatActivity } from "../types";

type Props = {
  activity: ChatActivity;
  /** verbosity === "detailed" 时为 true */
  defaultOpen: boolean;
  showTimestamp: boolean;
};

export default function MsgActivity({
  activity,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const hasDetail = Boolean(activity.detail);
  const [open, setOpen] = useState(defaultOpen && hasDetail);

  useEffect(() => {
    if (hasDetail) setOpen(defaultOpen);
  }, [defaultOpen, hasDetail]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  return (
    <div
      className={`msg-activity ${statusClass} ${openClass}`.trim()}
      data-kind={activity.kind}
    >
      <span className="msg-activity-kind">{activity.kind}</span>
      <div className="msg-activity-body">
        {hasDetail ? (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            aria-label={open ? t("chat.activityCollapse") : t("chat.activityExpand")}
            onClick={() => setOpen((v) => !v)}
          >
            <span className="msg-activity-title">{activity.title}</span>
            <ChevronDown
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        ) : (
          <span className="msg-activity-title">{activity.title}</span>
        )}
        {open && activity.detail ? (
          <pre className="msg-activity-detail">{activity.detail}</pre>
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

- [x] **Step 2: Commit**

```bash
git add apps/desktop/src/components/MsgActivity.tsx
git commit -m "$(cat <<'EOF'
feat(chat): add MsgActivity collapsible activity card

EOF
)"
```

---

### Task 3: 接线 `ActivityCards` + 样式

**Files:**
- Modify: `apps/desktop/src/components/ChatView.tsx`
- Modify: `apps/desktop/src/styles/chat.css`

- [x] **Step 1: 在 `ChatView.tsx` 顶部增加 import**

在现有 `MsgReasoning` import 旁加入：

```ts
import MsgActivity from "./MsgActivity";
```

- [x] **Step 2: 替换 `ActivityCards` 实现**

将现有 `ActivityCards`（约 411–446 行）整段替换为：

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
  const defaultOpen = prefs.verbosity === "detailed";
  return (
    <div className="msg-activities">
      {visible.map((a) => (
        <MsgActivity
          key={a.id}
          activity={a}
          defaultOpen={defaultOpen}
          showTimestamp={showTimestamps}
        />
      ))}
    </div>
  );
}
```

注意：删除原先在 `ActivityCards` 内直接渲染 `msg-activity-detail` 且仅 `verbosity === "detailed"` 才显示的逻辑——详情改由 `MsgActivity` 在展开时显示（normal 也可点开查看）。

- [x] **Step 3: 在 `chat.css` 的 `.msg-activity-title` 附近追加样式**

在 `.msg-activity-title` 规则之后、`.msg-activity-detail` 之前插入：

```css
.msg-activity-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  margin: 0;
  padding: 0;
  border: none;
  background: transparent;
  cursor: pointer;
  text-align: left;
  font: inherit;
  color: inherit;
}

.msg-activity-toggle:hover .msg-activity-title {
  color: var(--ink-soft, var(--ink));
}

.msg-activity-chevron {
  flex-shrink: 0;
  margin-left: auto;
  color: var(--ink-mute);
  transition: transform 0.15s ease;
}

.msg-activity.is-open .msg-activity-chevron {
  transform: rotate(180deg);
}

/* 脉冲点挂在 title 上；toggle 内 title 仍适用 */
.msg-activity.is-running .msg-activity-title::after {
  content: "";
  display: inline-block;
  width: 6px;
  height: 6px;
  margin-left: 6px;
  border-radius: 50%;
  background: var(--tone-blue, #2563eb);
  animation: msg-activity-pulse 1.1s ease-in-out infinite;
  vertical-align: middle;
}
```

若文件中已有完全相同的 `.msg-activity.is-running .msg-activity-title::after` 规则，**不要重复粘贴**——保留原有那一段即可，只新增 toggle / chevron / `is-open` 相关规则。

- [x] **Step 4: 类型检查**

```bash
cd frontend && npx tsc -b --pretty false
```

Expected: 无错误退出（exit 0）。

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/components/ChatView.tsx apps/desktop/src/styles/chat.css
git commit -m "$(cat <<'EOF'
feat(chat): collapse activity details by default

Wire MsgActivity into bubbles; detailed verbosity opens by default.
EOF
)"
```

---

### Task 4: 手动验收 + 收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-activity-cards-collapse-design.md`（状态）

- [x] **Step 1: 手动验收清单**

启动前端（或已有 Tauri 会话），在智能对话中触发至少一次带 `detail` 的工具/记忆活动（如 `memory_add`），核对：

1. **normal**：活动卡默认一行摘要；点击标题行展开可见完整 detail；再点可折叠。
2. **detailed**（偏好里切换）：默认展开 detail；仍可手动折叠。
3. 切回 **normal**：卡重新默认折叠（覆盖手动状态）。
4. 无 `detail` 的卡：无 chevron / 不可点折叠。
5. **思考块**：流式展开、结束后自动折叠，与改前一致。
6. running 卡：折叠态仍可见脉冲点。

- [x] **Step 2: 更新 spec 状态**

将 spec 头部：

```markdown
**状态:** 已批准
```

改为：

```markdown
**状态:** 已批准 / 已实现
```

- [x] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-13-activity-cards-collapse-design.md
git commit -m "$(cat <<'EOF'
docs: mark activity cards collapse spec implemented

EOF
)"
```

---

## Spec Coverage Checklist

| Spec 要求 | Task |
|-----------|------|
| 逐条折叠 | Task 2–3 |
| normal/compact 默认折叠 | Task 3 `defaultOpen` |
| detailed 默认展开 | Task 3 |
| 手动切换 | Task 2 button |
| verbosity 变化同步 | Task 2 `useEffect` |
| 无 detail 不可折叠 | Task 2 分支 |
| 不改思考 / 时间线 | 明确不改文件 |
| 验收项 | Task 4 |

## Placeholder / Consistency Self-Review

- 无 TBD；组件名统一为 `MsgActivity`；props 为 `defaultOpen` / `showTimestamp`。
- detail 在 normal 下可展开查看（相对旧逻辑「仅 detailed 才渲染 detail」的有意变更，与 spec C 一致）。
