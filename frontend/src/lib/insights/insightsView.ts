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
