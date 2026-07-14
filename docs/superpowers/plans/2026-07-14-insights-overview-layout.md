# Insights Overview Layout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为数据洞察增加默认「总览」Tab（3 KPI + 趋势 + 厂商花钱 Top），并收敛「模型用量」去掉与总览重复的 6 KPI / 整宽趋势 / 未计价横幅。

**Architecture:** 纯前端改造。复用 `get_usage_insights`；从 `InsightsPanel` 抽出可测的 view 常量与「厂商 Top N」助手；`ViewMode` 增加 `overview` 且默认选中；用量拉取条件包含 overview。样式对齐现有 amber glass + insights 网格。

**Tech Stack:** React 18、现有 i18n、insights.css、`node:test` 纯函数测试

**Spec:** `docs/superpowers/specs/2026-07-14-insights-overview-layout-design.md`

> **Plan status:** Completed on `main`（2026-07-14）。实现已落地；后续 polish（趋势空态 + `InsightsTrendChart` 抽取）同批收尾。

---

## File map

| 文件 | 职责 |
|------|------|
| `frontend/src/lib/insightsView.ts` | `ViewMode`、默认 Tab、是否拉 usage、厂商花钱 Top N |
| `frontend/src/lib/insightsView.test.ts` | 上述纯函数测试 |
| `frontend/src/i18n/messages.ts` | `insights.view.overview`、`insights.rank.more`、`insights.rank.providerSpend` 中英 |
| `frontend/src/components/InsightsPanel.tsx` | Tab / 默认 view / fetch / 总览 UI / 模型 Tab 收敛 |
| `frontend/src/styles/insights.css` | `.insights-overview-*`、三列 KPI、响应式网格 |

不做：后端 API、协作/Tracing 布局大改、图表库。

---

### Task 1: `insightsView` 纯模块 + 失败测试

**Files:**
- Create: `frontend/src/lib/insightsView.ts`
- Create: `frontend/src/lib/insightsView.test.ts`

- [ ] **Step 1: Write the failing test**

```ts
// frontend/src/lib/insightsView.test.ts
import assert from "node:assert/strict";
import test from "node:test";
import {
  DEFAULT_INSIGHTS_VIEW,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerSpendTop,
  type InsightsRankItem,
} from "./insightsView.ts";

test("default view is overview and sits first in tab order", () => {
  assert.equal(DEFAULT_INSIGHTS_VIEW, "overview");
  assert.deepEqual(INSIGHTS_VIEW_ORDER, [
    "overview",
    "models",
    "tools",
    "collab",
    "tracing",
  ]);
});

test("needsUsageInsights covers overview, models, tools only", () => {
  assert.equal(needsUsageInsights("overview"), true);
  assert.equal(needsUsageInsights("models"), true);
  assert.equal(needsUsageInsights("tools"), true);
  assert.equal(needsUsageInsights("collab"), false);
  assert.equal(needsUsageInsights("tracing"), false);
});

test("providerSpendTop sorts by cost then tokens and caps length", () => {
  const items: InsightsRankItem[] = [
    { kind: "provider", name: "a", calls: 1, tokens: 10, cost_usd: 1 },
    { kind: "provider", name: "b", calls: 1, tokens: 90, cost_usd: 5 },
    { kind: "provider", name: "c", calls: 1, tokens: 50, cost_usd: 5 },
    { kind: "provider", name: "d", calls: 1, tokens: 1, cost_usd: 0.1 },
  ];
  const top = providerSpendTop(items, 2);
  assert.equal(top.length, 2);
  assert.equal(top[0]?.name, "b"); // cost tie → higher tokens first
  assert.equal(top[1]?.name, "c");
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd frontend && node --experimental-strip-types --test src/lib/insightsView.test.ts
```

Expected: FAIL（模块不存在或导出缺失）

- [ ] **Step 3: Minimal implementation**

```ts
// frontend/src/lib/insightsView.ts
export type InsightsViewMode =
  | "overview"
  | "models"
  | "tools"
  | "collab"
  | "tracing";

export const DEFAULT_INSIGHTS_VIEW: InsightsViewMode = "overview";

export const INSIGHTS_VIEW_ORDER: readonly InsightsViewMode[] = [
  "overview",
  "models",
  "tools",
  "collab",
  "tracing",
] as const;

export function needsUsageInsights(view: InsightsViewMode): boolean {
  return view === "overview" || view === "models" || view === "tools";
}

export type InsightsRankItem = {
  kind: string;
  name: string;
  calls: number;
  tokens: number;
  cost_usd: number;
};

/** 按费用（估）降序，同费用再按 tokens；截断为 Top N。 */
export function providerSpendTop(
  items: InsightsRankItem[],
  n = 5,
): InsightsRankItem[] {
  return [...items]
    .sort(
      (a, b) =>
        b.cost_usd - a.cost_usd ||
        b.tokens - a.tokens ||
        b.calls - a.calls,
    )
    .slice(0, Math.max(0, n));
}
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cd frontend && node --experimental-strip-types --test src/lib/insightsView.test.ts
```

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/insightsView.ts frontend/src/lib/insightsView.test.ts
git commit -m "$(cat <<'EOF'
feat(frontend): add insights view helpers for overview-first tabs

EOF
)"
```

---

### Task 2: i18n 键

**Files:**
- Modify: `frontend/src/i18n/messages.ts`（`zh` 与 `en` 对象，约 insights.view 区块）

- [ ] **Step 1: 在中文 `zh` 增加键（紧挨现有 insights.view.*）**

```ts
"insights.view.overview": "总览",
"insights.rank.providerSpend": "厂商费用 Top",
"insights.rank.more": "查看明细",
"insights.empty.overview": "暂无用量数据。对话与工具调用会出现在这里。",
```

保留已有 `"insights.view.models"` 等。

- [ ] **Step 2: 在英文 `en` 对称增加**

```ts
"insights.view.overview": "Overview",
"insights.rank.providerSpend": "Provider cost top",
"insights.rank.more": "View details",
"insights.empty.overview": "No usage yet. Chat and tool calls will show up here.",
```

- [ ] **Step 3: 确认类型**

`MessageKey = keyof typeof zh` 会自动纳入。无需改其它文件。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(i18n): add Insights overview tab strings

EOF
)"
```

---

### Task 3: InsightsPanel — Tab、默认 view、fetch

**Files:**
- Modify: `frontend/src/components/InsightsPanel.tsx`

- [ ] **Step 1: 替换本地 ViewMode / VIEW_TABS / 默认 state / usage effect**

将顶部：

```ts
type ViewMode = "models" | "tools" | "collab" | "tracing";
```

改为：

```ts
import {
  DEFAULT_INSIGHTS_VIEW,
  INSIGHTS_VIEW_ORDER,
  needsUsageInsights,
  providerSpendTop,
  type InsightsViewMode,
} from "../lib/insightsView";

type ViewMode = InsightsViewMode;
```

`VIEW_TABS` 改为按 `INSIGHTS_VIEW_ORDER` 生成（Icon 映射）：

```ts
const VIEW_TAB_META: Record<
  ViewMode,
  { labelKey: MessageKey; Icon: typeof BarChart3 }
> = {
  overview: { labelKey: "insights.view.overview", Icon: LayoutDashboard },
  models: { labelKey: "insights.view.models", Icon: Cpu },
  tools: { labelKey: "insights.view.tools", Icon: Wrench },
  collab: { labelKey: "insights.view.collab", Icon: Network },
  tracing: { labelKey: "insights.view.tracing", Icon: Activity },
};

const VIEW_TABS = INSIGHTS_VIEW_ORDER.map((id) => ({
  id,
  ...VIEW_TAB_META[id],
}));
```

从 `lucide-react` 增加 `LayoutDashboard` 到现有 import。

```ts
const [view, setView] = useState<ViewMode>(DEFAULT_INSIGHTS_VIEW);
```

Usage `useEffect` 条件从：

```ts
if (!active || !isTauri() || (view !== "models" && view !== "tools")) return;
```

改为：

```ts
if (!active || !isTauri() || !needsUsageInsights(view)) return;
```

- [ ] **Step 2: Typecheck**

```bash
cd frontend && npx tsc -b --pretty false
```

Expected: 可能因尚未渲染 overview 分支而仍通过；若有错误只修类型。

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/InsightsPanel.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): default Insights to overview and fetch usage for it

EOF
)"
```

---

### Task 4: 总览 UI 区块

**Files:**
- Modify: `frontend/src/components/InsightsPanel.tsx`（`return` 内，error 之后、现有 `view === "models"` 之前）
- Modify: `frontend/src/styles/insights.css`

- [ ] **Step 1: 计算 overview 用 provider Top（面板内）**

```ts
const overviewProviderTop = useMemo(
  () => providerSpendTop(byProvider, 5),
  [byProvider],
);
const overviewProviderMax = Math.max(
  1,
  ...overviewProviderTop.map((r) => (r.cost_usd > 0 ? r.cost_usd : r.tokens)),
);
const overviewUseCost = overviewProviderTop.some((r) => r.cost_usd > 0);
```

- [ ] **Step 2: 插入 `view === "overview" && data` 区块**

结构（复用已有 `KpiCard`、`insights-chart`、hbar）：

```tsx
{view === "overview" && data && (
  <>
    {hasUnpriced && (
      <p className="insights-unpriced">
        <AlertTriangle size={14} strokeWidth={2.25} aria-hidden />
        {t("insights.unpriced")}
      </p>
    )}

    <div className="insights-kpis insights-kpis-overview">
      <KpiCard
        icon={<DollarSign size={16} strokeWidth={2.25} aria-hidden />}
        label={t("insights.kpi.cost")}
        value={formatCost(data.kpis.cost_usd)}
      />
      <KpiCard
        icon={<Coins size={16} strokeWidth={2.25} aria-hidden />}
        label={t("insights.kpi.tokens")}
        value={formatTokens(data.kpis.tokens)}
      />
      <KpiCard
        icon={<Activity size={16} strokeWidth={2.25} aria-hidden />}
        label={t("insights.kpi.calls")}
        value={String(data.kpis.calls)}
      />
    </div>

    <div className="insights-overview-grid">
      <div className="insights-chart-wrap insights-models-chart">
        {/* 与现有 models 趋势相同：metric tabs + bars 或 empty */}
      </div>

      <section className="insights-hbar-panel">
        <div className="insights-rank-title-row">
          <h3 className="insights-rank-title">
            <Building2 size={14} strokeWidth={2.25} aria-hidden />
            {t("insights.rank.providerSpend")}
          </h3>
          <button
            type="button"
            className="insights-more-btn"
            onClick={() => setView("models")}
          >
            {t("insights.rank.more")}
          </button>
        </div>
        {/* overviewProviderTop 条形列表；空则 insights-rank-empty */}
      </section>
    </div>

    {!data.series.length && overviewProviderTop.length === 0 && (
      <p className="insights-panel-hint">{t("insights.empty.overview")}</p>
    )}
  </>
)}
```

趋势块从现有 models 分支复制结构（metric tabs + series map）。若重复明显，可在同文件抽 `InsightsTrendChart` 小函数组件；不要新建图表依赖。

- [ ] **Step 3: CSS**

在 `insights.css` 追加：

```css
.insights-kpis-overview {
  grid-template-columns: repeat(3, minmax(0, 1fr));
}

.insights-overview-grid {
  display: grid;
  grid-template-columns: 1.5fr 1fr;
  gap: 12px;
  align-items: stretch;
  min-width: 0;
}

@media (max-width: 900px) {
  .insights-overview-grid {
    grid-template-columns: 1fr;
  }

  .insights-kpis-overview {
    grid-template-columns: 1fr;
  }
}

.insights-rank-title-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  margin-bottom: 8px;
}

.insights-rank-title-row .insights-rank-title {
  margin: 0;
}

.insights-more-btn {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--tone);
  font: inherit;
  font-size: 12px;
  font-weight: 650;
  cursor: pointer;
  padding: 4px 6px;
  border-radius: 8px;
}

.insights-more-btn:hover {
  background: color-mix(in srgb, var(--tone) 12%, transparent);
}
```

确认 `.insights-panel` 仍有 `overflow-x: hidden`。

- [ ] **Step 4: tsc**

```bash
cd frontend && npx tsc -b --pretty false
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/InsightsPanel.tsx frontend/src/styles/insights.css
git commit -m "$(cat <<'EOF'
feat(frontend): render Insights overview with KPIs, trend, and spend top

EOF
)"
```

---

### Task 5: 模型用量 Tab 收敛

**Files:**
- Modify: `frontend/src/components/InsightsPanel.tsx`（`view === "models"` 分支）
- Modify: `frontend/src/styles/insights.css`

按 spec 明确取舍：

- [ ] **Step 1: 删除 models 分支内**
  - 原 6 卡 `insights-kpis-models` 整块
  - `insights-models-grid` 内的整宽趋势 + 右侧厂商占比
  - `hasUnpriced` 横幅（只留总览）

- [ ] **Step 2: 顶部仅 2 个次要 KPI**

```tsx
<div className="insights-kpis insights-kpis-models-secondary">
  <KpiCard
    icon={<Layers size={16} strokeWidth={2.25} aria-hidden />}
    label={t("insights.kpi.models")}
    value={String(modelStats.modelCount)}
  />
  <KpiCard
    icon={<Bot size={16} strokeWidth={2.25} aria-hidden />}
    label={t("insights.kpi.agents")}
    value={String(modelStats.agentCount || data.kpis.active_agents)}
  />
</div>
```

CSS：

```css
.insights-kpis-models-secondary {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  max-width: 420px;
}
```

- [ ] **Step 3: 保留**
  - `insights-hbar-panel` 模型 Tokens 排行（`modelBars`）
  - `insights-ranks` 三列 RankList（provider / model / agent）
  - `modelsEmpty` hint

- [ ] **Step 4: tsc + 单元测试**

```bash
cd frontend && node --experimental-strip-types --test src/lib/insightsView.test.ts && npx tsc -b --pretty false
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/InsightsPanel.tsx frontend/src/styles/insights.css
git commit -m "$(cat <<'EOF'
refactor(frontend): slim Insights models tab after overview split

EOF
)"
```

---

### Task 6: 手工验收 + spec 状态

**Files:**
- Modify: `docs/superpowers/specs/2026-07-14-insights-overview-layout-design.md`（状态行）

- [ ] **Step 1: 在 Tauri/dev 中核对验收清单**

1. 进入洞察默认「总览」
2. 宽屏首屏可见 3 KPI + 趋势 + 厂商 Top
3. 切换月/季/年与 Agent 后总览数据更新
4. 「查看明细」进入模型用量
5. 模型 Tab 无 6 KPI / 无重复趋势 / 无未计价横幅
6. 工具 / 协作 / Tracing 仍可用
7. 触控板横滑不拖偏整页

- [ ] **Step 2: 将 spec 状态改为**

```markdown
**状态:** 已批准 / 已实现
```

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/specs/2026-07-14-insights-overview-layout-design.md
git commit -m "$(cat <<'EOF'
docs: mark Insights overview layout design as implemented

EOF
)"
```

---

## Spec coverage check

| Spec 要求 | Task |
|-----------|------|
| 默认总览 + 五 Tab | 1, 3 |
| 3 KPI + 趋势 + 厂商 Top | 4 |
| 未计价仅总览 | 4, 5 |
| 更多 → 模型 | 4 |
| 模型 Tab 去 6 KPI/趋势 | 5 |
| 复用 API / 含 overview fetch | 3 |
| overflow-x 保持 | 4（复用现有 CSS） |
| i18n | 2 |
| 协作/Tracing 不大改 | （不触碰其 JSX 主体） |

## Placeholder / 类型一致性

- `ViewMode` ≡ `InsightsViewMode`；`DEFAULT_INSIGHTS_VIEW` / `needsUsageInsights` / `providerSpendTop` 命名全计划一致
- i18n 键：`insights.view.overview`、`insights.rank.providerSpend`、`insights.rank.more`、`insights.empty.overview`
- 无 TBD；趋势 JSX 允许同文件小抽取
