# Observability S2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Insights 右栏按 `turn_id` 默认折叠下钻；聊天展示当前 `turn_id` 并一键复制 `session_id`/`turn_id` 诊断文本。

**Architecture:** 扩展已有 `get_trace_insights`：UsageDb `list_trace_events` 读出 `turn_id` → `TraceEvent` 透传；前端按 turn 分组折叠。聊天把 `RunStarted.run_id` 抬成 `currentTurnId` state（约定等于 turn_id），经 `ChatAgentInfo` 展示与复制。不新开 Tauri command，不做 OTEL。

**Tech Stack:** Rust (`memory` usage/trace_insights)、Tauri、React (`InsightsPanel`, `App`, `ChatAgentInfo`)、i18n、`insights.css`

**Spec:** [`docs/superpowers/specs/2026-07-14-observability-s2-design.md`](../specs/2026-07-14-observability-s2-design.md)

**Naming:** 禁止 `hermes` / `Hermes` 出现在代码与 UI。

---

## File map

| Path | Responsibility |
|------|----------------|
| `crates/agent-memory/src/usage/db.rs` | `TraceEventRow.turn_id`；`list_trace_events` SELECT |
| `crates/agent-memory/src/usage/trace_insights.rs` | `TraceEvent.turn_id`；usage→event 映射；chat-history 合成事件 `None` |
| `memory/tests/…` 或 `trace_insights` 单测 | 透传断言 |
| `apps/desktop/src/components/InsightsPanel.tsx` | 类型 + `groupEventsByTurn` + 折叠 UI |
| `apps/desktop/src/styles/insights.css` | `.insights-turn-group*` |
| `apps/desktop/src/App.tsx` | `currentTurnId` state；run_started / session 切换 |
| `apps/desktop/src/components/ChatRightPanel.tsx` | 下传 props |
| `apps/desktop/src/components/ChatAgentInfo.tsx` | 展示 + 复制诊断 |
| `apps/desktop/src/i18n/messages.ts` | 文案键 |
| `apps/desktop/src/lib/diagnosticContext.ts`（可选新建） | `formatDiagnosticContext(sessionId, turnId)` 纯函数便于测 |

---

## Task 1: Backend — `turn_id` 透出 `list_trace_events` / `TraceEvent`（TDD）

**Files:**
- Modify: `crates/agent-memory/src/usage/db.rs`
- Modify: `crates/agent-memory/src/usage/trace_insights.rs`
- Test: 在 `crates/agent-memory/src/usage/trace_insights.rs` 的 `#[cfg(test)]` 或 `memory/tests/` 追加（优先沿用现有 trace_insights 测试模块）

- [x] **Step 1: Write failing test**

在已有 trace_insights / usage 测试附近追加（用 tempfile UsageDb）：

```rust
#[test]
fn list_trace_events_includes_turn_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    let db = memory::UsageDb::new(path).unwrap();
    db.insert(memory::NewUsageEvent {
        ts: "2026-07-14T12:00:00Z".into(),
        kind: "llm".into(),
        name: "m".into(),
        agent_id: "a".into(),
        session_id: Some("sess-1".into()),
        turn_id: Some("turn-abc".into()),
        input_tokens: 1,
        output_tokens: 2,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 3,
        cost_usd: 0.01,
        cost_status: Some("estimated".into()),
        cost_source: None,
        pricing_version: None,
        billing_provider: None,
        billing_base_url: None,
        billing_mode: None,
        meta_json: None,
    })
    .unwrap();
    let rows = db.list_trace_events("sess-1", 50).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].turn_id.as_deref(), Some("turn-abc"));
}

#[test]
fn trace_event_from_usage_preserves_turn_id() {
    // 若存在 usage_rows → TraceEvent 的映射函数被测；否则通过 query_trace_insights
    // 最小：构造 TraceEventRow { turn_id: Some(...) } 走 merge 路径断言 TraceEvent.turn_id
}
```

（第二个测试按现有 `events_from_usage` / 类似私有函数访问调整；若私有，只测 `query_trace_insights` 公开路径。）

- [x] **Step 2: Run — expect FAIL**

```bash
cargo test -p memory list_trace_events_includes_turn_id -- --nocapture
```

Expected: compile error（无 `turn_id` 字段）或 assert fail。

- [x] **Step 3: Implement**

1. `TraceEventRow` 增加 `pub turn_id: Option<String>`
2. `list_trace_events` SQL：

```sql
SELECT id, ts, kind, name, agent_id,
       input_tokens, output_tokens, total_tokens, cost_usd, turn_id
FROM usage_events
WHERE session_id = ?1
ORDER BY ts ASC, rowid ASC
LIMIT {limit}
```

映射 `turn_id: r.get(9)?`

3. `TraceEvent` in `trace_insights.rs`：

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub turn_id: Option<String>,
```

4. 所有构造 `TraceEvent { ... }` 处：usage 行 → `turn_id: row.turn_id.clone()`；chat-history 合成 → `turn_id: None`
5. `merge_usage_into_spans`：在匹配到 usage 行时**同时拷贝** `turn_id`（否则 chat-history 主导时间线时 UI 全进「未标注」）

- [x] **Step 4: Tests PASS**

```bash
cargo test -p memory list_trace_events_includes_turn_id
cargo test -p memory --lib usage::trace_insights
```

- [x] **Step 5: Commit**

```bash
git add memory/src/usage/db.rs memory/src/usage/trace_insights.rs memory/tests/
git commit -m "$(cat <<'EOF'
feat(usage): expose turn_id on trace insights events

EOF
)"
```

---

## Task 2: Insights — `groupEventsByTurn` + 默认折叠 UI

**Files:**
- Modify: `apps/desktop/src/components/InsightsPanel.tsx`
- Modify: `apps/desktop/src/styles/insights.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: Add pure helper + unit-level assert via small extract (optional file)**

在 `InsightsPanel.tsx` 顶部（或 `apps/desktop/src/lib/traceTurnGroups.ts`）：

```ts
export type TraceEvent = { /* existing + */ turn_id?: string | null; total_tokens: number; cost_usd: number; /* ... */ };

export type TurnGroup = {
  turnKey: string; // "__none__" for unlabeled
  turn_id: string | null;
  events: TraceEvent[];
  tokens: number;
  cost_usd: number;
};

export function groupEventsByTurn(events: TraceEvent[]): TurnGroup[] {
  const order: string[] = [];
  const map = new Map<string, TurnGroup>();
  for (const ev of events) {
    const tid = ev.turn_id?.trim() ? ev.turn_id.trim() : null;
    const key = tid ?? "__none__";
    let g = map.get(key);
    if (!g) {
      g = { turnKey: key, turn_id: tid, events: [], tokens: 0, cost_usd: 0 };
      map.set(key, g);
      order.push(key);
    }
    g.events.push(ev);
    g.tokens += ev.total_tokens || 0;
    g.cost_usd += ev.cost_usd || 0;
  }
  // Move __none__ to end if present
  const keyed = order.filter((k) => k !== "__none__").concat(order.includes("__none__") ? ["__none__"] : []);
  return keyed.map((k) => map.get(k)!);
}

export function shortTurnId(id: string | null, unlabeled: string): string {
  if (!id) return unlabeled;
  return id.length <= 8 ? id : `${id.slice(0, 8)}…`;
}
```

若仓库无前端单测 runner，用手测 + 保证 helper 无副作用即可；优先抽到 `traceTurnGroups.ts` 便于日后测。

扩展 FE `TraceEvent` 类型加 `turn_id?: string | null`。

- [x] **Step 2: Replace flat timeline with collapsible groups**

在 `insights-trace-chain-panel` 中（约现有 `selectedTrace.events.map`）：

```tsx
const turnGroups = useMemo(
  () => (selectedTrace ? groupEventsByTurn(selectedTrace.events) : []),
  [selectedTrace],
);
const [expandedTurns, setExpandedTurns] = useState<Record<string, boolean>>({});
// Reset expanded when selectedTraceId changes:
useEffect(() => {
  setExpandedTurns({});
}, [selectedTraceId]);

// render:
<ol className="insights-turn-groups">
  {turnGroups.map((g) => {
    const open = !!expandedTurns[g.turnKey];
    return (
      <li key={g.turnKey} className="insights-turn-group">
        <button
          type="button"
          className="insights-turn-group-head"
          aria-expanded={open}
          onClick={() =>
            setExpandedTurns((s) => ({ ...s, [g.turnKey]: !s[g.turnKey] }))
          }
        >
          <span>{shortTurnId(g.turn_id, t("insights.trace.unlabeledTurn"))}</span>
          <span className="insights-turn-group-meta">
            {g.events.length} · {g.tokens} tok · ${g.cost_usd.toFixed(4)}
          </span>
        </button>
        {open && (
          <ol className="insights-trace-timeline">
            {g.events.map((ev, i) => (
              /* reuse existing event row markup */
            ))}
          </ol>
        )}
      </li>
    );
  })}
</ol>
```

**Default:** groups collapsed (`expandedTurns` 缺省 false）。

- [x] **Step 3: CSS + i18n**

`insights.css`:

```css
.insights-turn-groups { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 8px; }
.insights-turn-group-head {
  width: 100%;
  display: flex;
  justify-content: space-between;
  align-items: center;
  /* match existing insights panel controls; no new purple glow */
}
.insights-turn-group-meta { opacity: 0.75; font-size: 12px; }
```

i18n（en + zh 等全部 locale 块）：

```ts
"insights.trace.unlabeledTurn": "Unlabeled",
"insights.trace.turnGroup": "Turn",
// zh: "未标注" / "回合"
```

- [x] **Step 4: Manual / typecheck**

```bash
cd frontend && npx tsc --noEmit
```

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/components/InsightsPanel.tsx apps/desktop/src/styles/insights.css apps/desktop/src/i18n/messages.ts apps/desktop/src/lib/traceTurnGroups.ts
git commit -m "$(cat <<'EOF'
feat(insights): collapse trace timeline by turn_id

EOF
)"
```

---

## Task 3: Chat — `currentTurnId` + 复制诊断

**Files:**
- Create (可选): `apps/desktop/src/lib/diagnosticContext.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/ChatRightPanel.tsx`
- Modify: `apps/desktop/src/components/ChatAgentInfo.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: Formatter**

```ts
// apps/desktop/src/lib/diagnosticContext.ts
export function formatDiagnosticContext(sessionId: string, turnId: string): string {
  return `session_id=${sessionId}\nturn_id=${turnId}\n`;
}
```

- [x] **Step 2: App state**

- 将 `currentRunIdRef` 升为（或并存）`const [currentTurnId, setCurrentTurnId] = useState<string | null>(null)`
- 在 `run_started` 处理：`setCurrentTurnId(payload.run_id ?? null)`（保留 ref 若别处需要）
- `run_finished` / `done`：**不要**清空 turnId
- 切换 session / 新会话 / 清空聊天：`setCurrentTurnId(null)`
- 传入 `ChatRightPanel`：`sessionId={sessionId}`（已有）、`turnId={currentTurnId}`

- [x] **Step 3: ChatAgentInfo UI**

Props 增加：

```ts
sessionId?: string | null;
turnId?: string | null;
```

Usage 卡片下：

```tsx
{turnId && sessionId ? (
  <div className="chat-agent-turn">
    <span>{t("chat.rightPanel.turnLabel")}</span>
    <code>{turnId.length > 8 ? `${turnId.slice(0, 8)}…` : turnId}</code>
    <button
      type="button"
      onClick={() =>
        navigator.clipboard.writeText(formatDiagnosticContext(sessionId, turnId))
      }
    >
      {t("chat.rightPanel.copyDiagnostic")}
    </button>
  </div>
) : null}
```

`ChatRightPanel` 透传 `sessionId` + `turnId`。

i18n：

```ts
"chat.rightPanel.turnLabel": "Turn",
"chat.rightPanel.copyDiagnostic": "Copy diagnostic",
// zh: "回合" / "复制诊断"
```

- [x] **Step 4: Check**

```bash
cd frontend && npx tsc --noEmit
```

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/App.tsx apps/desktop/src/components/ChatRightPanel.tsx \
  apps/desktop/src/components/ChatAgentInfo.tsx apps/desktop/src/lib/diagnosticContext.ts \
  apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(chat): show turn_id and copy diagnostic context

EOF
)"
```

---

## Task 4: 验收 + 文档状态

- [x] **Step 1: Regressions**

```bash
cargo test -p memory list_trace_events_includes_turn_id
cargo test -p memory --lib usage::trace_insights
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
cd frontend && npx tsc --noEmit
```

- [x] **Step 2: Spec / plan checkbox**

将 `docs/superpowers/specs/2026-07-14-observability-s2-design.md` 状态改为 `已批准 / S2 已实现`；计划 Tasks 勾选。

- [x] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: mark observability S2 implemented

EOF
)"
```

---

## Spec coverage self-check

| Spec | Task |
|------|------|
| TraceEvent / list 透传 turn_id | Task 1 |
| Insights 默认按 turn 折叠；未标注桶 | Task 2 |
| 聊天展示 + 复制 session/turn | Task 3 |
| 无新 Tauri command / 无 OTEL / 左栏仍 session=Trace | 全计划遵守 |
| 复制不附带 log 正文 | Task 3 仅 formatDiagnosticContext IDs |

**Placeholder scan:** 无 TBD。  
**Type consistency:** `turn_id` / `TurnGroup` / `formatDiagnosticContext` / `currentTurnId === run_id`。
