export type ContextUsageSegmentId =
  | "system"
  | "tools"
  | "mcp"
  | "memory"
  | "skills"
  | "recall"
  | "subagent"
  | "conversation";

export type ContextUsageSegment = {
  id: ContextUsageSegmentId | string;
  tokens: number;
  count?: number;
};

export type ContextUsageSnapshot = {
  contextWindow: number;
  totalTokens: number;
  segments: ContextUsageSegment[];
  updatedAt: number;
};

export const SEGMENT_ORDER: ContextUsageSegmentId[] = [
  "system",
  "tools",
  "mcp",
  "memory",
  "skills",
  "recall",
  "subagent",
  "conversation",
];

/** CSS 变量名（不含 var()） */
export const SEGMENT_TONE: Record<string, string> = {
  system: "--ink-mute",
  tools: "--tone-purple",
  mcp: "--tone-pink",
  memory: "--tone-green",
  skills: "--tone-amber",
  recall: "--tone-cyan",
  subagent: "--tone-indigo",
  conversation: "--tone-orange",
};

export function formatTokenCount(n: number): string {
  if (!Number.isFinite(n) || n < 0) return "0";
  if (n < 1000) return String(Math.round(n));
  if (n < 1_000_000) {
    const k = n / 1000;
    const s = k >= 100 || Number.isInteger(k) ? String(Math.round(k)) : k.toFixed(1);
    return `${s.replace(/\.0$/, "")}K`;
  }
  const m = n / 1_000_000;
  return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
}

export function usagePercent(used: number, window: number): number {
  if (window <= 0 || used <= 0) return 0;
  return Math.min(99, Math.round((used / window) * 100));
}

export function visibleSegments(snap: ContextUsageSnapshot): ContextUsageSegment[] {
  return snap.segments
    .filter((s) => s.tokens > 0)
    .slice()
    .sort((a, b) => b.tokens - a.tokens);
}

export function resolveContextWindow(
  modelWindow: number | null | undefined,
  snapWindow: number | null | undefined,
): number {
  if (modelWindow && modelWindow > 0) return modelWindow;
  if (snapWindow && snapWindow > 0) return snapWindow;
  return 128_000;
}

export function normalizeContextUsageEvent(payload: {
  context_window?: number;
  total_tokens?: number;
  segments?: Array<{ id: string; tokens: number; count?: number | null }>;
  updated_at?: number;
}): ContextUsageSnapshot {
  return {
    contextWindow: payload.context_window ?? 0,
    totalTokens: payload.total_tokens ?? 0,
    updatedAt: payload.updated_at ?? 0,
    segments: (payload.segments ?? []).map((segment) => {
      const normalized: ContextUsageSegment = {
        id: segment.id,
        tokens: segment.tokens,
      };
      if (segment.count != null) {
        normalized.count = segment.count;
      }
      return normalized;
    }),
  };
}
