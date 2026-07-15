/**
 * 助手消息时间线拼装：按事件交错 reasoning / activity / surface。
 */

import type {
  ChatActivity,
  ChatMessage,
  ChatTimelineSegment,
  UiSurface,
} from "../types";

function ensureSegments(m: ChatMessage): ChatTimelineSegment[] {
  return [...(m.segments ?? [])];
}

function elapsedSec(startedAtMs: number, endedAtMs = Date.now()): number {
  return Math.max(0.1, Math.round(((endedAtMs - startedAtMs) / 1000) * 10) / 10);
}

/** 各 reasoning 段 durationSec 求和（无则 undefined） */
export function sumReasoningDurations(
  segments: ChatTimelineSegment[] | undefined,
): number | undefined {
  if (!segments?.length) return undefined;
  let sum = 0;
  let any = false;
  for (const seg of segments) {
    if (seg.type === "reasoning" && seg.durationSec != null && seg.durationSec > 0) {
      sum += seg.durationSec;
      any = true;
    }
  }
  return any ? Math.round(sum * 10) / 10 : undefined;
}

/**
 * 同回合展示用：把多段 reasoning 合并成一块（放在首个 reasoning 位置），
 * activity / surface 保持相对顺序。末段仍开放（无 duration）则合并块也不封口。
 */
export function coalesceReasoningSegments(
  segments: ChatTimelineSegment[] | undefined,
): ChatTimelineSegment[] | undefined {
  if (!segments?.length) return segments;
  const reasoning = segments.filter(
    (s): s is Extract<ChatTimelineSegment, { type: "reasoning" }> =>
      s.type === "reasoning",
  );
  if (reasoning.length <= 1) return segments;

  const first = reasoning[0]!;
  const last = reasoning[reasoning.length - 1]!;
  const lastOpen = last.durationSec == null || last.durationSec <= 0;
  const merged: ChatTimelineSegment = {
    type: "reasoning",
    id: first.id,
    text: reasoning.map((r) => r.text).join(""),
    at: first.at,
    ...(lastOpen
      ? {}
      : { durationSec: sumReasoningDurations(segments) }),
  };

  let emitted = false;
  const out: ChatTimelineSegment[] = [];
  for (const seg of segments) {
    if (seg.type === "reasoning") {
      if (!emitted) {
        out.push(merged);
        emitted = true;
      }
      continue;
    }
    out.push(seg);
  }
  return out;
}

/**
 * 封口最近一条尚无 duration 的 reasoning（按段 at 墙钟）。
 * @param endedAt 封口时刻（ms）
 * @param durationSec 可选显式耗时；缺省用 endedAt - seg.at
 */
export function sealOpenReasoning(
  m: ChatMessage,
  endedAt: number = Date.now(),
  durationSec?: number,
): ChatMessage {
  const segments = ensureSegments(m);
  let target = -1;
  for (let i = segments.length - 1; i >= 0; i -= 1) {
    const seg = segments[i];
    if (seg?.type === "reasoning" && (seg.durationSec == null || seg.durationSec <= 0)) {
      target = i;
      break;
    }
  }
  if (target < 0) {
    const summed = sumReasoningDurations(segments);
    if (summed == null) return m;
    return { ...m, reasoningDurationSec: summed };
  }
  const last = segments[target];
  if (last?.type !== "reasoning") return m;
  const dur =
    durationSec != null && durationSec > 0
      ? durationSec
      : elapsedSec(last.at, endedAt);
  segments[target] = { ...last, durationSec: dur };
  return {
    ...m,
    segments,
    reasoningDurationSec: sumReasoningDurations(segments),
  };
}

/** 追加 reasoning；新开段前封口上一段；同步拼接 m.reasoning */
export function applyReasoningDelta(
  m: ChatMessage,
  delta: string,
  at: number = Date.now(),
): ChatMessage {
  if (!delta) return m;
  let segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type === "reasoning") {
    segments[segments.length - 1] = {
      ...last,
      text: last.text + delta,
    };
    return {
      ...m,
      segments,
      reasoning: (m.reasoning ?? "") + delta,
    };
  }
  const sealed = sealOpenReasoning({ ...m, segments }, at);
  segments = ensureSegments(sealed);
  segments.push({
    type: "reasoning",
    id: `r-${at}-${segments.length}`,
    text: delta,
    at,
  });
  return {
    ...sealed,
    segments,
    reasoning: (sealed.reasoning ?? "") + delta,
  };
}

/** 工具活动 upsert；新 id 入列前封口当前 reasoning；完成态写 durationSec */
export function applyActivityUpsert(
  m: ChatMessage,
  activity: ChatActivity,
): ChatMessage {
  const at = activity.at ?? Date.now();
  const isNew = !(m.activities ?? []).some((a) => a.id === activity.id);
  const base = isNew ? sealOpenReasoning(m, at) : m;

  const segments = ensureSegments(base);
  const activities = [...(base.activities ?? [])];
  const existing = activities.findIndex((a) => a.id === activity.id);
  if (existing >= 0) {
    const prev = activities[existing]!;
    const preservedAt = prev.at ?? at;
    let durationSec = activity.durationSec ?? prev.durationSec;
    const status = activity.status ?? prev.status;
    if (
      (status === "done" || status === "error") &&
      (durationSec == null || durationSec <= 0) &&
      preservedAt != null
    ) {
      durationSec = elapsedSec(preservedAt, Date.now());
    }
    activities[existing] = {
      ...prev,
      ...activity,
      at: preservedAt,
      durationSec,
    };
  } else {
    let durationSec = activity.durationSec;
    if (
      (activity.status === "done" || activity.status === "error") &&
      (durationSec == null || durationSec <= 0)
    ) {
      durationSec = elapsedSec(at, Date.now());
    }
    activities.push({ ...activity, at, durationSec });
    segments.push({ type: "activity", id: activity.id, at });
  }
  return {
    ...base,
    activities,
    segments,
    reasoningDurationSec: sumReasoningDurations(segments) ?? base.reasoningDurationSec,
  };
}

/** A2UI surface upsert；新 messageId 时 push surface 段 */
export function applySurfaceUpsert(
  m: ChatMessage,
  surface: UiSurface,
  at: number = Date.now(),
): ChatMessage {
  const surfaces = [...(m.uiSurfaces ?? [])];
  const idx = surfaces.findIndex((s) => s.messageId === surface.messageId);
  if (idx >= 0) surfaces[idx] = surface;
  else surfaces.push(surface);

  const segments = ensureSegments(m);
  const hasSeg = segments.some(
    (s) => s.type === "surface" && s.id === surface.messageId,
  );
  if (!hasSeg) {
    segments.push({ type: "surface", id: surface.messageId, at });
  }
  return { ...m, uiSurfaces: surfaces, segments };
}
