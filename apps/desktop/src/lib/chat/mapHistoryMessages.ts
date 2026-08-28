/** 将 `get_chat_history` DTO 映射为 UI `ChatMessage[]`。 */
import type {
  ChatActivity,
  ChatActivityKind,
  ChatHistoryActivityDto,
  ChatHistoryMessageDto,
  ChatMessage,
  ChatTimelineSegment,
  UiSurface,
} from "../../types";
import { elapsedSecSince } from "./elapsedSec.ts";

const ACTIVITY_KINDS = new Set<ChatActivityKind>([
  "tool",
  "skill",
  "mcp",
  "hook",
  "memory",
  "status",
]);

function sumReasoningDurations(
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

/** 同轮多段 assistant（工具循环）合并为一条气泡，取最完整 timeline。 */
export function coalesceConsecutiveAssistants(
  messages: ChatHistoryMessageDto[],
): ChatHistoryMessageDto[] {
  const out: ChatHistoryMessageDto[] = [];
  for (const m of messages) {
    if (m.role !== "assistant") {
      out.push(m);
      continue;
    }
    const prev = out[out.length - 1];
    if (!prev || prev.role !== "assistant") {
      out.push({
        ...m,
        activities: [...(m.activities ?? [])],
      });
      continue;
    }

    const contents = [prev.content, m.content]
      .map((c) => (c ?? "").trim())
      .filter(Boolean);
    prev.content = contents.join("\n\n");
    prev.activities = mergeActivities(prev.activities ?? [], m.activities ?? []);
    if ((m.segments?.length ?? 0) >= (prev.segments?.length ?? 0) && m.segments?.length) {
      prev.segments = m.segments;
    }
    if ((m.uiSurfaces?.length ?? 0) >= (prev.uiSurfaces?.length ?? 0) && m.uiSurfaces?.length) {
      prev.uiSurfaces = m.uiSurfaces;
    }
    if (m.reasoning?.trim()) {
      prev.reasoning = m.reasoning;
    }
  }
  return out;
}

function mergeActivities(
  a: ChatHistoryActivityDto[],
  b: ChatHistoryActivityDto[],
): ChatHistoryActivityDto[] {
  const byId = new Map<string, ChatHistoryActivityDto>();
  const order: string[] = [];
  for (const act of [...a, ...b]) {
    const id = act.id || `${act.title}-${order.length}`;
    const prev = byId.get(id);
    if (!prev) {
      byId.set(id, { ...act, id });
      order.push(id);
      continue;
    }
    byId.set(id, {
      ...prev,
      ...act,
      id,
      input: act.input ?? prev.input,
      output: act.output ?? prev.output,
      media: act.media ?? prev.media,
      status: act.status ?? prev.status,
    });
  }
  return order.map((id) => byId.get(id)!);
}

function asTimelineSegments(raw: unknown): ChatTimelineSegment[] | undefined {
  if (!Array.isArray(raw) || raw.length === 0) return undefined;
  const out: ChatTimelineSegment[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const seg = item as Record<string, unknown>;
    const type = seg.type;
    const id = typeof seg.id === "string" ? seg.id : "";
    const at = typeof seg.at === "number" ? seg.at : 0;
    if (type === "reasoning") {
      out.push({
        type: "reasoning",
        id: id || `r-${out.length}`,
        text: typeof seg.text === "string" ? seg.text : "",
        at,
        durationSec:
          typeof seg.durationSec === "number"
            ? seg.durationSec
            : typeof seg.duration_sec === "number"
              ? seg.duration_sec
              : undefined,
      });
    } else if (type === "activity") {
      out.push({ type: "activity", id: id || `a-${out.length}`, at });
    } else if (type === "surface") {
      out.push({ type: "surface", id: id || `s-${out.length}`, at });
    }
  }
  return out.length > 0 ? out : undefined;
}

/** 用相邻段 `at` 墙钟差补全历史缺失的 durationSec。 */
export function enrichTimelineDurations(
  segments: ChatTimelineSegment[],
): ChatTimelineSegment[] {
  return segments.map((seg, i) => {
    if (seg.type !== "reasoning") return seg;
    if (seg.durationSec != null && seg.durationSec > 0) return seg;
    const next = segments[i + 1];
    if (next && next.at > seg.at) {
      const durationSec = Math.round(((next.at - seg.at) / 1000) * 10) / 10;
      if (durationSec > 0) return { ...seg, durationSec };
    }
    return seg;
  });
}

function enrichActivityDurations(
  activities: ChatActivity[],
  segments: ChatTimelineSegment[] | undefined,
): ChatActivity[] {
  if (!segments?.length) return activities;
  return activities.map((act) => {
    if (act.durationSec != null && act.durationSec > 0) return act;
    const idx = segments.findIndex(
      (s) => s.type === "activity" && s.id === act.id,
    );
    if (idx < 0) return act;
    const at = segments[idx]!.at;
    const next = segments[idx + 1];
    let durationSec = act.durationSec;
    if (next && next.at > at) {
      durationSec = Math.round(((next.at - at) / 1000) * 10) / 10;
    }
    return {
      ...act,
      at: act.at ?? at,
      durationSec: durationSec && durationSec > 0 ? durationSec : undefined,
    };
  });
}

function mapActivity(a: ChatHistoryActivityDto): ChatActivity {
  const kind = ACTIVITY_KINDS.has(a.kind as ChatActivityKind)
    ? (a.kind as ChatActivityKind)
    : "tool";
  const status =
    a.status === "running" ||
    a.status === "done" ||
    a.status === "error" ||
    a.status === "interrupted"
      ? a.status
      : undefined;
  const media = Array.isArray(a.media)
    ? a.media
        .map((item) => {
          const mediaKind =
            item.kind === "image" ||
            item.kind === "video" ||
            item.kind === "audio" ||
            item.kind === "html"
              ? item.kind
              : null;
          const path = typeof item.path === "string" ? item.path.trim() : "";
          if (!mediaKind || !path) return null;
          return { kind: mediaKind, path };
        })
        .filter(Boolean) as NonNullable<ChatActivity["media"]>
    : undefined;
  return {
    id: a.id,
    kind,
    title: a.title,
    input: a.input ?? undefined,
    output: a.output ?? undefined,
    status,
    media: media && media.length > 0 ? media : undefined,
  };
}

/**
 * 历史快照中的 running 已经失去对应的当前进程执行实例。
 * 将其收敛为终态并冻结耗时，避免应用重启后继续显示和累计“运行中”。
 */
export function settleRestoredActivities(
  messages: ChatMessage[],
  settledAt = Date.now(),
): ChatMessage[] {
  return messages.map((message) => {
    if (!message.activities?.some((activity) => activity.status === "running")) {
      return message;
    }
    return {
      ...message,
      activities: message.activities.map((activity) => {
        if (activity.status !== "running") return activity;
        const durationSec =
          activity.durationSec ??
          (activity.at != null
            ? elapsedSecSince(activity.at, settledAt)
            : undefined);
        return {
          ...activity,
          status: "interrupted" as const,
          durationSec,
        };
      }),
    };
  });
}

export function mapHistoryMessages(messages: ChatHistoryMessageDto[]): ChatMessage[] {
  return coalesceConsecutiveAssistants(messages)
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => {
      let segments = asTimelineSegments(m.segments);
      if (segments) {
        segments = enrichTimelineDurations(segments);
      }
      let activities: ChatActivity[] | undefined =
        m.activities && m.activities.length > 0
          ? m.activities.map(mapActivity)
          : undefined;
      if (activities) {
        activities = enrichActivityDurations(activities, segments);
      }
      const uiSurfaces: UiSurface[] | undefined =
        Array.isArray(m.uiSurfaces) && m.uiSurfaces.length > 0
          ? m.uiSurfaces.map((s) => {
              const status =
                s.status === "resolved" || s.status === "cancelled"
                  ? s.status
                  : ("active" as const);
              return {
                messageId: s.messageId,
                activityType: s.activityType,
                operations: Array.isArray(s.operations) ? s.operations : [],
                status,
                interrupts: s.interrupts,
              };
            })
          : undefined;
      const reasoningDurationSec = sumReasoningDurations(segments);
      return {
        id: m.id,
        role: m.role as "user" | "assistant",
        content: m.content,
        reasoning: m.reasoning ?? undefined,
        activities,
        segments,
        uiSurfaces,
        reasoningDurationSec,
      };
    });
}
