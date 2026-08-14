# Insights Collaboration 2D Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 洞察面板新增「协作」Tab：按月/季/年展示近期编排列表+步骤条，以及基于 `orchestration` 遥测边的 SVG 聚合图。

**Architecture:** `memory` 扩展 `OrchestrationDb` 按时间窗列表查询，并新增 `collab_insights`（或 `UsageDb` 方法）聚合 `kind=orchestration` 且 `phase=end` 的边。Tauri 暴露单一命令 `get_collaboration_insights`；`InsightsPanel` 增加用量|协作切换，复用 period/Agent 筛选，图用纯 SVG。

**Tech Stack:** Rust / rusqlite / chrono、Tauri 2 invoke、React + 现有 i18n / `insights.css`

**Spec:** `docs/superpowers/specs/2026-07-13-insights-collaboration-2d-design.md`

> **Plan status:** Completed on `main`（2026-07-13）。实现已落地；勿再按本计划重复开工。

---

## File map

| 文件 | 职责 |
|------|------|
| `memory/src/orchestration_db.rs` | `list_in_period`（created_at 窗 + agent + limit） |
| `memory/src/collab_insights.rs` | `CollaborationInsights` 类型 + `query_collaboration_insights` |
| `memory/src/usage_db.rs` | 导出 period 边界辅助（`period_window`）供 collab 复用 |
| `memory/src/lib.rs` | 导出 |
| `memory/tests/collab_insights_test.rs` | TDD：列表过滤、图聚合、agent 过滤、calls 不膨胀 |
| `frontend/src-tauri/src/config_commands.rs` | `get_collaboration_insights` |
| `frontend/src-tauri/src/lib.rs` | 注册命令 |
| `apps/desktop/src/components/InsightsPanel.tsx` | Tab + 协作 UI + SVG |
| `apps/desktop/src/styles/insights.css` | 协作布局样式 |
| `apps/desktop/src/i18n/messages.ts` | 中英文案 |

**对齐现有：** `get_usage_insights` 在 `config_commands.rs`；`InsightsPanel` 已 `invoke("get_usage_insights", { args: { period, as_of, agent_id } })`。

---

### Task 1: 导出 period 时间窗 + OrchestrationDb 列表查询（TDD）

**Files:**
- Modify: `memory/src/usage_db.rs`
- Modify: `memory/src/orchestration_db.rs`
- Modify: `memory/tests/orchestration_db_test.rs`
- Modify: `memory/src/lib.rs`（若需导出新类型）

- [ ] **Step 1: 在 `usage_db` 增加可复用窗口 API**

把现有私有 `period_bounds` / `parse_as_of` 包一层公开函数（名称固定）：

```rust
/// 返回 period 半开区间 `[start, end)` 的 RFC3339 UTC 字符串（与 query_insights 一致）。
pub fn period_window(
    period: UsagePeriod,
    as_of: Option<&str>,
) -> anyhow::Result<(String, String)> {
    let as_of_dt = parse_as_of(as_of)?;
    let (start, end, _) = period_bounds(period, &as_of_dt);
    Ok((start, end))
}
```

- [ ] **Step 2: 写失败测试 `list_in_period`**

追加到 `memory/tests/orchestration_db_test.rs`：

```rust
#[test]
fn list_in_period_filters_by_created_at_and_agent() {
    let dir = tempfile::TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    let db = memory::OrchestrationDb::open_default().unwrap();

    // 手动插入两行不同 created_at / parent —— 若 create() 总是 now，
    // 则用 SQL 或测试辅助：先 create 再 UPDATE created_at。
    let id_in = db
        .create(memory::NewOrchestration {
            parent_agent_id: "alice".into(),
            session_id: None,
            goal: "in-window".into(),
            steps: vec![memory::NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
        })
        .unwrap();
    let id_out = db
        .create(memory::NewOrchestration {
            parent_agent_id: "bob".into(),
            session_id: None,
            goal: "out".into(),
            steps: vec![memory::NewOrchestrationStep {
                role: "r".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
        })
        .unwrap();

    // 将 id_in 钉在窗内、id_out 钉在窗外（示例时间）
    db.debug_set_created_at(&id_in, "2026-07-10T12:00:00.000Z").unwrap();
    db.debug_set_created_at(&id_out, "2026-05-01T12:00:00.000Z").unwrap();

    let rows = db
        .list_in_period(
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
            Some("alice"),
            50,
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id_in);
    let steps = db.list_steps(&rows[0].id).unwrap();
    assert_eq!(steps.len(), 1);
}
```

若不想暴露 `debug_set_created_at`，可用 `#[cfg(test)]` 方法，或测试里 `Connection` 不可用则改为：在 `orchestration_db` 增加仅测试可见的 `#[cfg(test)] pub fn set_created_at_for_test`。

- [ ] **Step 3: 实现**

```rust
impl OrchestrationDb {
    /// 按 created_at ∈ [start, end) 列出编排，可选 parent_agent_id，按 created_at DESC，limit（0→50）。
    pub fn list_in_period(
        &self,
        start: &str,
        end: &str,
        parent_agent_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<OrchestrationRow>> { /* ... */ }

    #[cfg(test)]
    pub fn set_created_at_for_test(&self, id: &str, created_at: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE orchestrations SET created_at = ?2, updated_at = ?2 WHERE id = ?1",
            rusqlite::params![id, created_at],
        )?;
        Ok(())
    }
}
```

- [ ] **Step 4: 测试通过并 commit**

```bash
cargo test -p memory --test orchestration_db_test
git add memory/src/usage_db.rs memory/src/orchestration_db.rs memory/tests/orchestration_db_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): list orchestrations in period window

EOF
)"
```

---

### Task 2: `query_collaboration_insights`（TDD）

**Files:**
- Create: `memory/src/collab_insights.rs`
- Create: `memory/tests/collab_insights_test.rs`
- Modify: `memory/src/lib.rs`

- [ ] **Step 1: 类型与查询签名（写进测试与实现）**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationInsights {
    pub orchestrations: Vec<CollaborationOrchestration>,
    pub graph: CollaborationGraph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationOrchestration {
    pub id: String,
    pub goal: String,
    pub status: String,
    pub parent_agent_id: String,
    pub session_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
    pub result_summary: Option<String>,
    pub steps: Vec<CollaborationStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationStep {
    pub seq: i64,
    pub role: String,
    pub agent_id: Option<String>,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationGraph {
    pub nodes: Vec<CollaborationNode>,
    pub edges: Vec<CollaborationEdge>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationNode {
    pub id: String,
    pub label: String,
    pub kind: String, // "agent" | "role"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollaborationEdge {
    pub from: String,
    pub to: String,
    pub weight: i64,
}

#[derive(Debug, Clone)]
pub struct CollaborationInsightsQuery {
    pub period: UsagePeriod,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

pub const COLLAB_LIST_LIMIT: usize = 50;
pub const COLLAB_OUTPUT_MAX_BYTES: usize = 2 * 1024;

pub fn query_collaboration_insights(
    q: CollaborationInsightsQuery,
) -> anyhow::Result<CollaborationInsights> {
    let (start, end) = crate::usage_db::period_window(q.period, q.as_of.as_deref())?;
    let agent = q.agent_id.filter(|s| !s.is_empty());
    let orch_db = OrchestrationDb::open_default()?;
    let rows = orch_db.list_in_period(&start, &end, agent.as_deref(), COLLAB_LIST_LIMIT)?;
    let mut orchestrations = Vec::new();
    for row in rows {
        let steps = orch_db.list_steps(&row.id)?;
        orchestrations.push(CollaborationOrchestration {
            id: row.id,
            goal: row.goal,
            status: row.status,
            parent_agent_id: row.parent_agent_id,
            session_id: row.session_id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            finished_at: row.finished_at,
            error: row.error,
            result_summary: row.result_summary,
            steps: steps
                .into_iter()
                .map(|s| CollaborationStep {
                    seq: s.seq,
                    role: s.role,
                    agent_id: s.agent_id,
                    status: s.status,
                    output: s.output.map(|o| truncate_utf8(&o, COLLAB_OUTPUT_MAX_BYTES)),
                    error: s.error,
                })
                .collect(),
        });
    }
    let graph = build_graph_from_usage(&start, &end, agent.as_deref())?;
    Ok(CollaborationInsights {
        orchestrations,
        graph,
    })
}
```

`build_graph_from_usage`：

```rust
fn build_graph_from_usage(
    start: &str,
    end: &str,
    agent_id: Option<&str>,
) -> anyhow::Result<CollaborationGraph> {
    let db = UsageDb::open_default()?;
    // SELECT meta_json FROM usage_events
    // WHERE kind='orchestration' AND ts >= start AND ts < end
    // 在 Rust 解析 JSON：phase=="end"，取 from/to；缺字段则跳过
    // agent 过滤：from==agent || to==agent
    // HashMap<(from,to), weight> → edges；nodes 从端点生成
    // label: 若 id.starts_with("role:") 则 strip 前缀，kind=role，否则 kind=agent、label=id
}
```

可在 `UsageDb` 增加：

```rust
pub fn list_orchestration_meta(
    &self,
    start: &str,
    end: &str,
) -> anyhow::Result<Vec<String>> // meta_json 文本列表
```

- [ ] **Step 2: 失败测试**

`memory/tests/collab_insights_test.rs`：

```rust
#[test]
fn graph_counts_end_phase_only_and_filters_agent() {
    let dir = tempfile::TempDir::new().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    let usage = memory::UsageDb::open_default().unwrap();
    let meta_end = serde_json::json!({
        "from": "alice", "to": "role:r", "phase": "end", "ok": true
    })
    .to_string();
    let meta_start = serde_json::json!({
        "from": "alice", "to": "role:r", "phase": "start"
    })
    .to_string();
    usage
        .insert(memory::NewUsageEvent {
            ts: "2026-07-10T12:00:00Z".into(),
            kind: "orchestration".into(),
            name: "orchestration_step".into(),
            agent_id: "alice".into(),
            session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: Some(meta_end),
        })
        .unwrap();
    usage
        .insert(memory::NewUsageEvent {
            ts: "2026-07-10T12:01:00Z".into(),
            kind: "orchestration".into(),
            name: "orchestration_step".into(),
            agent_id: "alice".into(),
            session_id: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: Some(meta_start),
        })
        .unwrap();

    let insights = memory::query_collaboration_insights(memory::CollaborationInsightsQuery {
        period: memory::UsagePeriod::Month,
        as_of: Some("2026-07-13T12:00:00Z".into()),
        agent_id: Some("alice".into()),
    })
    .unwrap();
    assert_eq!(insights.graph.edges.len(), 1);
    assert_eq!(insights.graph.edges[0].weight, 1);
    assert_eq!(insights.graph.edges[0].to, "role:r");

    // orchestration 不计入 calls
    let usage_insights = usage
        .query_insights(memory::UsageInsightsQuery {
            period: memory::UsagePeriod::Month,
            as_of: Some("2026-07-13T12:00:00Z".into()),
            agent_id: None,
        })
        .unwrap();
    assert_eq!(usage_insights.kpis.calls, 0);
}
```

另测：空库返回空列表/空图不报错。

- [ ] **Step 3: 实现至测试通过**

```bash
cargo test -p memory --test collab_insights_test
```

- [ ] **Step 4: Commit**

```bash
git add memory/src/collab_insights.rs memory/src/lib.rs memory/src/usage_db.rs \
  memory/tests/collab_insights_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): query collaboration insights for Insights tab

EOF
)"
```

---

### Task 3: Tauri 命令 `get_collaboration_insights`

**Files:**
- Modify: `frontend/src-tauri/src/config_commands.rs`
- Modify: `frontend/src-tauri/src/lib.rs`

- [ ] **Step 1: 参数与命令（对齐 `get_usage_insights`）**

```rust
#[derive(Debug, Clone, serde::Deserialize)]
pub struct CollaborationInsightsArgs {
    pub period: String,
    pub as_of: Option<String>,
    pub agent_id: Option<String>,
}

#[tauri::command]
pub async fn get_collaboration_insights(
    args: CollaborationInsightsArgs,
) -> Result<memory::CollaborationInsights, String> {
    let period = match args.period.to_lowercase().as_str() {
        "month" => memory::UsagePeriod::Month,
        "quarter" => memory::UsagePeriod::Quarter,
        "year" => memory::UsagePeriod::Year,
        other => return Err(format!("invalid period: {other}")),
    };
    let agent_id = normalize_agent_id(args.agent_id);
    memory::query_collaboration_insights(memory::CollaborationInsightsQuery {
        period,
        as_of: args.as_of,
        agent_id,
    })
    .map_err(|e| e.to_string())
}
```

在 `lib.rs` 的 `invoke_handler` 列表中注册 `config_commands::get_collaboration_insights`（紧挨 `get_usage_insights`）。

- [ ] **Step 2: Check**

```bash
cargo check -p astro-agent
```

- [ ] **Step 3: Commit**

```bash
git add frontend/src-tauri/src/config_commands.rs frontend/src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(tauri): expose get_collaboration_insights command

EOF
)"
```

---

### Task 4: i18n 文案

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: 中英键（固定 key）**

```ts
// zh + en 均需：
"insights.view.usage": "用量" / "Usage",
"insights.view.collab": "协作" / "Collaboration",
"insights.collab.empty": "暂无编排记录。可在对话中调用 orchestration_run。" / "No orchestrations yet. Call orchestration_run in chat.",
"insights.collab.listTitle": "近期编排" / "Recent orchestrations",
"insights.collab.graphTitle": "协作图" / "Collaboration graph",
"insights.collab.steps": "步骤" / "Steps",
"insights.collab.noSelection": "选择一条编排查看步骤" / "Select an orchestration to view steps",
```

- [ ] **Step 2: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(i18n): add Insights collaboration tab strings

EOF
)"
```

---

### Task 5: InsightsPanel 协作 UI + SVG

**Files:**
- Modify: `apps/desktop/src/components/InsightsPanel.tsx`
- Modify: `apps/desktop/src/styles/insights.css`

- [ ] **Step 1: 状态与加载**

```tsx
type ViewMode = "usage" | "collab";
const [view, setView] = useState<ViewMode>("usage");
const [collab, setCollab] = useState<CollaborationInsights | null>(null);
const [selectedId, setSelectedId] = useState<string | null>(null);

// usage effect: 仅 view==="usage" 时 invoke get_usage_insights（保持现有）
// collab effect: view==="collab" 时 invoke get_collaboration_insights，依赖 [active, period, agentId, view]
```

类型与返回 JSON 字段对齐 Task 2（camelCase：确认 serde 默认 snake 则前端用 snake，或 `#[serde(rename_all = "camelCase")]`——**写死：与 `UsageInsights` 一致用 snake_case 字段名**，前端类型用 `parent_agent_id` 等）。

- [ ] **Step 2: Tab UI**

在 period tabs **上方或左侧**加：

```tsx
<div className="insights-view-tabs" role="tablist">
  <button role="tab" className={view==="usage"?"active":""} onClick={() => setView("usage")}>
    {t("insights.view.usage")}
  </button>
  <button role="tab" className={view==="collab"?"active":""} onClick={() => setView("collab")}>
    {t("insights.view.collab")}
  </button>
</div>
```

AgentPicker + period 两 Tab 共用。

- [ ] **Step 3: 协作布局**

- 左：`orchestrations.map` 列表；点击设 `selectedId`；状态色 class `status-done|failed|running|…`
- 下方或左下：选中项 steps 条（pill + 箭头）；可 `<details>` 看 output/error
- 右：`<CollabGraphSvg nodes={...} edges={...} />` 同文件内函数组件

SVG 布局（圆形）：

```tsx
function CollabGraphSvg({ nodes, edges }: { nodes: ...; edges: ... }) {
  const w = 320, h = 240, cx = w/2, cy = h/2, R = 80;
  const pos = new Map(nodes.map((n, i) => {
    const a = (2 * Math.PI * i) / Math.max(nodes.length, 1) - Math.PI/2;
    return [n.id, { x: cx + R * Math.cos(a), y: cy + R * Math.sin(a) }];
  }));
  const maxW = Math.max(1, ...edges.map(e => e.weight));
  return (
    <svg viewBox={`0 0 ${w} ${h}`} className="insights-collab-graph">
      {edges.map(e => {
        const a = pos.get(e.from), b = pos.get(e.to);
        if (!a || !b) return null;
        const sw = 1 + (3 * e.weight) / maxW;
        return (
          <g key={`${e.from}->${e.to}`}>
            <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} strokeWidth={sw} className="insights-collab-edge" />
            <title>{`${e.from} → ${e.to}: ${e.weight}`}</title>
          </g>
        );
      })}
      {nodes.map(n => {
        const p = pos.get(n.id)!;
        return (
          <g key={n.id}>
            <circle cx={p.x} cy={p.y} r={18} className={`insights-collab-node kind-${n.kind}`} />
            <text x={p.x} y={p.y+4} textAnchor="middle" className="insights-collab-label">
              {n.label.length > 8 ? n.label.slice(0,7)+"…" : n.label}
            </text>
          </g>
        );
      })}
    </svg>
  );
}
```

空态：`orchestrations.length===0` 显示 `t("insights.collab.empty")`。

- [ ] **Step 4: CSS**

在 `insights.css` 增加：`.insights-view-tabs`、`.insights-collab-layout`（grid 1.2fr 1fr）、`.insights-collab-list-item`、status 色、`.insights-collab-edge` / node / label、窄屏 `grid-template-columns: 1fr`。

- [ ] **Step 5: 手工/编译检查**

```bash
cd frontend && npx tsc --noEmit
```

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/components/InsightsPanel.tsx apps/desktop/src/styles/insights.css
git commit -m "$(cat <<'EOF'
feat(frontend): Insights collaboration tab with SVG graph

EOF
)"
```

---

### Task 6: 回归与验收对照

- [ ] **Step 1: 命令**

```bash
cargo test -p memory --test orchestration_db_test
cargo test -p memory --test collab_insights_test
cargo test -p memory --test usage_db_test
cargo check -p memory -p astro-agent
cd frontend && npx tsc --noEmit
```

- [ ] **Step 2: 对照 spec 验收清单**

1. 空态不报错  
2. 有 orchestration 时列表/步骤正确  
3. 有 phase=end 遥测时图有边  
4. period/agent 刷新  
5. 用量 calls 不含 orchestration  
6. 用量 Tab 行为不变  

- [ ] **Step 3: 若全绿，本计划完成**（合并走 finishing-a-development-branch）

---

## Self-review（plan vs spec）

| Spec | Task |
|------|------|
| get_collaboration_insights | Task 2–3 |
| 列表 orchestration.db + period/agent/limit/output 截断 | Task 1–2 |
| 图 usage phase=end + agent 边过滤 | Task 2 |
| 用量\|协作 Tab + 复用筛选 | Task 5 |
| SVG 无物理库 | Task 5 |
| i18n / 空态 | Task 4–5 |
| calls 不膨胀 | Task 2 测试 |
| 非目标 3D/独立导航 | 未列入 ✅ |

**类型名全程一致：** `CollaborationInsights` / `query_collaboration_insights` / `get_collaboration_insights`。
