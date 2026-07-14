/** 将 Trace 事件按 turn_id 分组，供 Insights 调用链折叠展示。 */

export type TraceEventForTurnGroup = {
  turn_id?: string | null;
  total_tokens: number;
  cost_usd: number;
};

export type TurnGroup<T extends TraceEventForTurnGroup = TraceEventForTurnGroup> = {
  /** `"__none__"` 表示未标注回合 */
  turnKey: string;
  turn_id: string | null;
  events: T[];
  tokens: number;
  cost_usd: number;
};

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

export function shortTurnId(id: string | null, unlabeled: string): string {
  if (!id) return unlabeled;
  return id.length <= 8 ? id : `${id.slice(0, 8)}…`;
}
