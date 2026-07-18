/** 将 Trace 事件按回合分组，供 Insights 调用链折叠展示。 */

export type TraceEventForTurnGroup = {
  kind?: string;
  name?: string;
  turn_id?: string | null;
  total_tokens: number;
  cost_usd: number;
  input?: string | null;
  output?: string | null;
};

export type TurnGroup<T extends TraceEventForTurnGroup = TraceEventForTurnGroup> = {
  /** 分组键；无 turn_id 时为 `__user_N` / `__none__` */
  turnKey: string;
  turn_id: string | null;
  events: T[];
  tokens: number;
  cost_usd: number;
};

/** 按 turn_id 聚合（无 id 归入 `__none__`）。 */
export function groupEventsByTurn<T extends TraceEventForTurnGroup>(
  events: T[],
): TurnGroup<T>[] {
  const order: string[] = [];
  const map = new Map<string, TurnGroup<T>>();
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
  const keyed = order
    .filter((k) => k !== "__none__")
    .concat(order.includes("__none__") ? ["__none__"] : []);
  return keyed.map((k) => map.get(k)!);
}

/** 有用户消息时按对话回合分组，否则按 turn_id。 */
export function groupEventsForTraceDisplay<T extends TraceEventForTurnGroup>(
  events: T[],
): TurnGroup<T>[] {
  if (events.some((e) => e.kind === "user")) {
    return groupEventsByUserTurns(events);
  }
  return groupEventsByTurn(events);
}

/**
 * 按用户消息边界分组：一条用户输入 + 其后的工具/LLM，直到下一条用户消息。
 * 比纯 turn_id 更贴近对话阅读；turn_id 仍保留在组上（取组内首个非空）。
 */
export function groupEventsByUserTurns<T extends TraceEventForTurnGroup>(
  events: T[],
): TurnGroup<T>[] {
  const groups: TurnGroup<T>[] = [];
  let current: TurnGroup<T> | null = null;

  const startGroup = (seedTurnId: string | null) => {
    const tid = seedTurnId?.trim() ? seedTurnId.trim() : null;
    const g: TurnGroup<T> = {
      turnKey: tid ?? `__user_${groups.length}`,
      turn_id: tid,
      events: [],
      tokens: 0,
      cost_usd: 0,
    };
    groups.push(g);
    current = g;
    return g;
  };

  for (const ev of events) {
    const kind = ev.kind ?? "";
    if (kind === "user" || !current) {
      startGroup(ev.turn_id ?? null);
    }
    const g = current!;
    g.events.push(ev);
    g.tokens += ev.total_tokens || 0;
    g.cost_usd += ev.cost_usd || 0;
    if (!g.turn_id && ev.turn_id?.trim()) {
      g.turn_id = ev.turn_id.trim();
      if (g.turnKey.startsWith("__user_")) {
        g.turnKey = g.turn_id;
      }
    }
  }
  return groups;
}

export function shortTurnId(id: string | null, unlabeled: string): string {
  if (!id) return unlabeled;
  return id.length <= 8 ? id : `${id.slice(0, 8)}…`;
}

function oneLine(text: string, maxLen: number): string {
  const s = text.replace(/\s+/g, " ").trim();
  if (!s) return "";
  if (s.length <= maxLen) return s;
  return `${s.slice(0, Math.max(1, maxLen - 1))}…`;
}

/** 回合标题：优先用户问题，其次工具/模型名，最后才用短 turn_id。 */
export function turnGroupTitle(
  events: TraceEventForTurnGroup[],
  turnId: string | null,
  unlabeled: string,
  turnPrefix: string,
  maxLen = 56,
): string {
  const user = events.find((e) => e.kind === "user");
  const userText = oneLine(user?.output || user?.input || "", maxLen);
  if (userText) return userText;

  const named = events.find((e) => {
    const n = (e.name || "").trim();
    if (!n) return false;
    if (e.kind === "user" || e.kind === "assistant") return false;
    if (n === "assistant" || n === "user") return false;
    return true;
  });
  if (named?.name) return oneLine(named.name, maxLen);

  if (turnId) return `${turnPrefix} ${shortTurnId(turnId, unlabeled)}`;
  return unlabeled;
}

/** 会话列表标题：首条用户预览，否则短 session_id。 */
export function traceSessionTitle(title: string | undefined, sessionId: string): string {
  const t = oneLine(title || "", 72);
  if (t) return t;
  if (sessionId.length <= 22) return sessionId;
  return `${sessionId.slice(0, 10)}…${sessionId.slice(-6)}`;
}
