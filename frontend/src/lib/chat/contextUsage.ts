export type ContextUsageSegmentId =
  | "system"
  | "tools"
  | "agents"
  | "mcp"
  | "memory"
  | "skills"
  | "recall"
  | "subagent"
  | "conversation";

export type ContextUsageItem = {
  id: string;
  label: string;
  tokens: number;
};

export type ContextUsageSegment = {
  id: ContextUsageSegmentId | string;
  tokens: number;
  count?: number;
  /** 分类下的明细（工具 / skill / 记忆文件等） */
  items?: ContextUsageItem[];
};

export type ContextUsageSnapshot = {
  contextWindow: number;
  totalTokens: number;
  segments: ContextUsageSegment[];
  updatedAt: number;
  /** 后端建议执行会话级 /compact（占用临界）；流式中仅 toast，不自动拆分 */
  recommendCompact?: boolean;
};

export const SEGMENT_ORDER: ContextUsageSegmentId[] = [
  "system",
  "tools",
  "agents",
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
  agents: "--tone-indigo",
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

/** 真实占用百分比；窗口未知时返回 0，满窗为 100（不虚构 128K）。 */
export function usagePercent(used: number, window: number): number {
  if (window <= 0 || used <= 0) return 0;
  return Math.min(100, Math.round((used / window) * 100));
}

export function visibleSegments(snap: ContextUsageSnapshot): ContextUsageSegment[] {
  return snap.segments
    .filter((s) => s.tokens > 0 || (s.items?.length ?? 0) > 0)
    .slice()
    .sort((a, b) => b.tokens - a.tokens);
}

/**
 * 解析展示用上下文窗口。
 * 优先用后端本轮快照（与 occupancy 同源），其次模型元数据；未知返回 0，绝不默认真造 128K。
 */
export function resolveContextWindow(
  modelWindow: number | null | undefined,
  snapWindow: number | null | undefined,
): number {
  if (snapWindow && snapWindow > 0) return snapWindow;
  if (modelWindow && modelWindow > 0) return modelWindow;
  return 0;
}

/** 展示用窗口：快照自带 > 传入 windowTokens > 0 */
export function displayContextWindow(
  snapshot: Pick<ContextUsageSnapshot, "contextWindow"> | null | undefined,
  windowTokens?: number | null,
): number {
  return resolveContextWindow(windowTokens, snapshot?.contextWindow);
}

type RawSegment = {
  id: string;
  tokens: number;
  count?: number | null;
  items?: Array<{ id?: string; label?: string; tokens?: number } | null> | null;
};

export function normalizeContextUsageEvent(payload: {
  context_window?: number;
  total_tokens?: number;
  segments?: RawSegment[];
  updated_at?: number;
  recommend_compact?: boolean;
  recommendCompact?: boolean;
}): ContextUsageSnapshot {
  const recommendCompact =
    payload.recommend_compact === true || payload.recommendCompact === true;
  return {
    contextWindow: payload.context_window ?? 0,
    totalTokens: payload.total_tokens ?? 0,
    updatedAt: payload.updated_at ?? 0,
    recommendCompact: recommendCompact || undefined,
    segments: (payload.segments ?? []).map((segment) => {
      const normalized: ContextUsageSegment = {
        id: segment.id,
        tokens: segment.tokens,
      };
      if (segment.count != null) {
        normalized.count = segment.count;
      }
      const items = (segment.items ?? [])
        .filter((it): it is NonNullable<typeof it> => !!it && (it.tokens ?? 0) > 0)
        .map((it) => ({
          id: it.id ?? it.label ?? "item",
          label: it.label || it.id || "item",
          tokens: it.tokens ?? 0,
        }));
      if (items.length > 0) {
        normalized.items = items;
      }
      return normalized;
    }),
  };
}
