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

/** 追加 reasoning；若末段非 reasoning 则新开段；同步拼接 m.reasoning */
export function applyReasoningDelta(
  m: ChatMessage,
  delta: string,
  at: number = Date.now(),
): ChatMessage {
  if (!delta) return m;
  const segments = ensureSegments(m);
  const last = segments[segments.length - 1];
  if (last?.type === "reasoning") {
    segments[segments.length - 1] = {
      ...last,
      text: last.text + delta,
    };
  } else {
    segments.push({
      type: "reasoning",
      id: `r-${at}-${segments.length}`,
      text: delta,
      at,
    });
  }
  return {
    ...m,
    segments,
    reasoning: (m.reasoning ?? "") + delta,
  };
}

/** 工具活动 upsert；新 id 时 push activity 段 */
export function applyActivityUpsert(
  m: ChatMessage,
  activity: ChatActivity,
): ChatMessage {
  const at = activity.at ?? Date.now();
  const segments = ensureSegments(m);
  const activities = [...(m.activities ?? [])];
  const idx = activities.findIndex((a) => a.id === activity.id);
  if (idx >= 0) {
    activities[idx] = {
      ...activities[idx],
      ...activity,
      at: activities[idx].at ?? at,
    };
  } else {
    activities.push({ ...activity, at });
    segments.push({ type: "activity", id: activity.id, at });
  }
  return { ...m, activities, segments };
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

/** 封口最近一条 reasoning 段（写入 durationSec），并同步 message 级耗时 */
export function sealOpenReasoning(
  m: ChatMessage,
  durationSec?: number,
): ChatMessage {
  const segments = ensureSegments(m);
  let target = -1;
  for (let i = segments.length - 1; i >= 0; i -= 1) {
    if (segments[i]?.type === "reasoning") {
      target = i;
      break;
    }
  }
  if (target < 0) {
    if (durationSec == null || durationSec <= 0) return m;
    return { ...m, reasoningDurationSec: durationSec };
  }
  const last = segments[target];
  if (last?.type !== "reasoning") return m;
  segments[target] = {
    ...last,
    durationSec: durationSec ?? last.durationSec,
  };
  return {
    ...m,
    segments,
    reasoningDurationSec: durationSec ?? m.reasoningDurationSec ?? last.durationSec,
  };
}
