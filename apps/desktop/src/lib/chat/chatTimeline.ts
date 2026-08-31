/**
 * 助手消息时间线拼装：按事件交错 reasoning / text / activity / surface。
 */

import type {
  ChatActivity,
  ChatMessage,
  ChatTimelineSegment,
  UiSurface,
} from "../../types";
import { elapsedSecSince } from "./elapsedSec.ts";

function ensureSegments(m: ChatMessage): ChatTimelineSegment[] {
  return [...(m.segments ?? [])];
}

/** 各 reasoning 段 durationSec 求和（无则 undefined） */
export function sumReasoningDurations(
  segments: ChatTimelineSegment[] | undefined,
): number | undefined {
  if (!segments?.length) return undefined;
  let sum = 0;
  let any = false;
  for (const seg of segments) {
    if (
      seg.type === "reasoning" &&
      seg.durationSec != null &&
      seg.durationSec > 0
    ) {
      sum += seg.durationSec;
      any = true;
    }
  }
  return any ? Math.round(sum * 10) / 10 : undefined;
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
    if (
      seg?.type === "reasoning" &&
      (seg.durationSec == null || seg.durationSec <= 0)
    ) {
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
      : elapsedSecSince(last.at, endedAt);
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

/** 追加正文；只合并相邻正文，跨思考或工具后新开一段。 */
export function applyTextDelta(
  m: ChatMessage,
  delta: string,
  at: number = Date.now(),
): ChatMessage {
  if (!delta) return m;
  let segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type === "text") {
    segments[segments.length - 1] = {
      ...last,
      text: last.text + delta,
    };
    return {
      ...m,
      segments,
      content: m.content + delta,
    };
  }
  const sealed = sealOpenReasoning({ ...m, segments }, at);
  segments = ensureSegments(sealed);
  segments.push({
    type: "text",
    id: `txt-${at}-${segments.length}`,
    text: delta,
    at,
  });
  return {
    ...sealed,
    segments,
    content: sealed.content + delta,
  };
}

function reconcileTextualSegments(
  segments: ChatTimelineSegment[],
  type: "reasoning" | "text",
  canonical: string,
  at: number,
): ChatTimelineSegment[] {
  const indexes = segments.flatMap((segment, index) =>
    segment.type === type ? [index] : [],
  );
  if (indexes.length === 0) {
    if (!canonical) return segments;
    return [
      ...segments,
      type === "reasoning"
        ? { type, id: `r-${at}-${segments.length}`, text: canonical, at }
        : { type, id: `txt-${at}-${segments.length}`, text: canonical, at },
    ];
  }

  const lastIndex = indexes[indexes.length - 1]!;
  const prefix = indexes
    .slice(0, -1)
    .map((index) => {
      const segment = segments[index]!;
      return segment.type === type ? segment.text : "";
    })
    .join("");
  if (canonical.startsWith(prefix)) {
    const replacement = canonical.slice(prefix.length);
    return segments.flatMap((segment, index) => {
      if (index !== lastIndex) return [segment];
      if (!replacement) return [];
      if (segment.type === "reasoning") {
        const { durationSec: _, ...open } = segment;
        return [{ ...open, text: replacement }];
      }
      return segment.type === "text"
        ? [{ ...segment, text: replacement }]
        : [segment];
    });
  }

  const firstIndex = indexes[0]!;
  return segments.flatMap((segment, index) => {
    if (segment.type !== type) return [segment];
    if (index !== firstIndex || !canonical) return [];
    if (segment.type === "reasoning") {
      const { durationSec: _, ...open } = segment;
      return [{ ...open, text: canonical }];
    }
    return [{ ...segment, text: canonical }];
  });
}

/** 用恢复快照校正文案，同时尽可能保留既有事件边界。 */
export function reconcileText(
  m: ChatMessage,
  canonical: string,
  at: number = Date.now(),
): ChatMessage {
  return {
    ...m,
    content: canonical,
    segments: reconcileTextualSegments(
      ensureSegments(m),
      "text",
      canonical,
      at,
    ),
  };
}

/**
 * Build a read-only timeline projection whose textual segments match the
 * canonical message aggregates. Live/recovered events can occasionally miss
 * the final text delta even though `content` already contains the complete
 * answer; rendering the raw segments would then truncate timeline view.
 */
export function projectCanonicalTimelineSegments(
  message: ChatMessage,
): ChatTimelineSegment[] {
  const segments = ensureSegments(message);
  const canonicalText = message.content;
  if (!canonicalText) return segments;

  // History coalescing inserts paragraph separators between assistant rounds,
  // while their timeline segments retain the original per-round text. Match
  // each existing text span in order instead of comparing raw concatenation.
  // If any span cannot be located, keep the timeline untouched rather than
  // collapsing it into categorized-view order.
  let cursor = 0;
  for (const segment of segments) {
    if (segment.type !== "text") continue;
    const needle = segment.text.trim();
    if (!needle) continue;
    const index = canonicalText.indexOf(needle, cursor);
    if (index < 0) return segments;
    cursor = index + needle.length;
  }

  const suffix = canonicalText.slice(cursor);
  if (!suffix.trim()) return segments;

  const fallbackAt = segments[segments.length - 1]?.at ?? 0;
  const lastIndex = segments.length - 1;
  const last = segments[lastIndex];
  if (last?.type === "text") {
    segments[lastIndex] = { ...last, text: last.text + suffix };
    return segments;
  }
  return [
    ...segments,
    {
      type: "text",
      id: `txt-canonical-${message.id}`,
      text: suffix,
      at: fallbackAt,
    },
  ];
}

/** Replace recovered reasoning with one canonical segment while preserving non-reasoning order. */
export function reconcileReasoning(
  m: ChatMessage,
  canonical: string,
  at: number = Date.now(),
): ChatMessage {
  const segments = reconcileTextualSegments(
    ensureSegments(m),
    "reasoning",
    canonical,
    at,
  );
  return {
    ...m,
    reasoning: canonical,
    segments,
    reasoningDurationSec: sumReasoningDurations(segments),
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
      durationSec = elapsedSecSince(preservedAt, Date.now());
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
      durationSec = elapsedSecSince(at, Date.now());
    }
    activities.push({ ...activity, at, durationSec });
    segments.push({ type: "activity", id: activity.id, at });
  }
  return {
    ...base,
    activities,
    segments,
    reasoningDurationSec:
      sumReasoningDurations(segments) ?? base.reasoningDurationSec,
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

export function parseActivityOperations(contentJson?: string): unknown[] {
  if (!contentJson?.trim()) return [];
  try {
    const parsed = JSON.parse(contentJson) as unknown;
    if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed))
      return [];
    const operations = (parsed as { operations?: unknown }).operations;
    return Array.isArray(operations) ? operations : [];
  } catch {
    return [];
  }
}
