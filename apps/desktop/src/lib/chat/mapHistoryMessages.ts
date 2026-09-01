/** 将 `get_chat_history` DTO 映射为 UI `ChatMessage[]`。 */
import type {
  ChatActivity,
  ChatActivityKind,
  ChatActivityStatus,
  ChatHistoryActivityDto,
  ChatMessage,
  ChatTimelineSegment,
  StoredResponseItemDto,
  UiSurface,
} from "../../types";
import { elapsedSecSince } from "./elapsedSec.ts";
import {
  isLiveActivityStatus,
  resolveToolActivityStatus,
} from "./toolActivityStatus.ts";

const ACTIVITY_KINDS = new Set<ChatActivityKind>([
  "tool",
  "skill",
  "mcp",
  "hook",
  "memory",
  "status",
]);
const ACTIVITY_STATUSES = new Set<ChatActivityStatus>([
  "waiting",
  "running",
  "retrying",
  "done",
  "partial",
  "error",
  "declined",
  "interrupted",
]);

type JsonRecord = Record<string, unknown>;

type HistoryBubble = {
  id: string;
  role: "user" | "assistant";
  content: string;
  reasoning?: string | null;
  activities?: ChatHistoryActivityDto[];
  segments?: ChatTimelineSegment[] | null;
  uiSurfaces?: UiSurface[] | null;
};

function asRecord(value: unknown): JsonRecord | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as JsonRecord)
    : null;
}

function itemText(item: JsonRecord): string {
  if ((item.type === "message" || item.type === "agent_message") && Array.isArray(item.content)) {
    return item.content
      .map((part) => asRecord(part)?.text)
      .filter((text): text is string => typeof text === "string")
      .join("\n");
  }
  if (item.type === "reasoning") {
    return [...(Array.isArray(item.content) ? item.content : []), ...(Array.isArray(item.summary) ? item.summary : [])]
      .map((part) => asRecord(part)?.text)
      .filter((text): text is string => typeof text === "string")
      .join("\n");
  }
  if (typeof item.output === "string") return item.output;
  if (Array.isArray(item.output)) {
    return item.output
      .map((part) => asRecord(part)?.text)
      .filter((text): text is string => typeof text === "string")
      .join("\n");
  }
  if (item.type === "tool_search_output" && Array.isArray(item.tools)) {
    return JSON.stringify(item.tools);
  }
  return "";
}

function itemMetadata(item: JsonRecord): JsonRecord | null {
  return asRecord(item.internal_chat_message_metadata_passthrough);
}

function mediaFromMetadata(metadata: JsonRecord | null): ChatHistoryActivityDto["media"] {
  const raw = metadata?.astro_media;
  if (!Array.isArray(raw)) return undefined;
  const media = raw.flatMap((entry) => {
    const asset = asRecord(entry);
    const reference = asRecord(asset?.reference);
    const path = reference?.workspace_path ?? reference?.data_url ?? reference?.remote_uri;
    let kind = asset?.kind;
    if (kind === "file" && typeof path === "string" && /\.html?$/i.test(path)) kind = "html";
    if (!(["image", "video", "audio", "html"] as unknown[]).includes(kind) || typeof path !== "string" || !path) return [];
    return [{ kind: kind as "image" | "video" | "audio" | "html", path }];
  });
  return media.length > 0 ? media : undefined;
}

function responseItemsToBubbles(items: StoredResponseItemDto[]): HistoryBubble[] {
  const bubbles: HistoryBubble[] = [];
  const ensureAssistant = (id: string): HistoryBubble => {
    const last = bubbles[bubbles.length - 1];
    if (last?.role === "assistant") return last;
    const next: HistoryBubble = { id, role: "assistant", content: "", activities: [] };
    bubbles.push(next);
    return next;
  };
  const attachOutput = (
    id: string,
    callId: string | undefined,
    title: string | undefined,
    output: string,
    media: ChatHistoryActivityDto["media"],
    status: ChatActivityStatus,
  ) => {
    const assistant = ensureAssistant(id);
    const activity = callId
      ? assistant.activities?.find((candidate) => candidate.id === callId)
      : assistant.activities?.find((candidate) => candidate.output == null);
    if (activity) {
      if (callId) activity.id = callId;
      if (title) activity.title = title;
      activity.output = output;
      activity.status = status;
      if (media) activity.media = media;
      return;
    }
    assistant.activities ??= [];
    assistant.activities.push({
      id: callId ?? "unknown",
      kind: "tool",
      title: title ?? "tool",
      output,
      status,
      media,
    });
  };

  for (const stored of items) {
    const item = asRecord(stored.item);
    if (!item) continue;
    const metadata = itemMetadata(item);
    const segments = metadata?.astro_timeline_v1 as ChatTimelineSegment[] | undefined;
    const uiSurfaces = metadata?.astro_surfaces_v1 as UiSurface[] | undefined;
    const media = mediaFromMetadata(metadata);
    const type = item.type;
    if (type === "message" && (item.role === "user" || item.role === "assistant")) {
      bubbles.push({ id: stored.id, role: item.role, content: itemText(item), activities: [], segments, uiSurfaces });
    } else if (type === "agent_message") {
      bubbles.push({ id: stored.id, role: "assistant", content: itemText(item), activities: [], segments, uiSurfaces });
    } else if (type === "reasoning") {
      const assistant = ensureAssistant(stored.id);
      assistant.reasoning = itemText(item) || assistant.reasoning;
      if (segments) assistant.segments = segments;
      if (uiSurfaces) assistant.uiSurfaces = uiSurfaces;
    } else if (type === "function_call" || type === "custom_tool_call" || type === "tool_search_call") {
      const assistant = ensureAssistant(stored.id);
      const callId = typeof item.call_id === "string" ? item.call_id : stored.id;
      const title = typeof item.name === "string" ? item.name : "tool_search";
      const input = typeof item.arguments === "string" ? item.arguments : typeof item.input === "string" ? item.input : JSON.stringify(item.arguments ?? {});
      assistant.activities ??= [];
      assistant.activities.push({ id: callId, kind: "tool", title, input, status: "running" });
    } else if (type === "local_shell_call" || type === "web_search_call") {
      const assistant = ensureAssistant(stored.id);
      const title = type === "local_shell_call" ? "local_shell" : "web_search";
      const callId = typeof item.call_id === "string"
        ? item.call_id
        : typeof item.id === "string"
          ? item.id
          : stored.id;
      const inputValue = item.action ?? {};
      const input = typeof inputValue === "string" ? inputValue : JSON.stringify(inputValue);
      const status = resolveToolActivityStatus(
        typeof item.status === "string" ? item.status : "completed",
        "",
      );
      assistant.activities ??= [];
      assistant.activities.push({ id: callId, kind: "tool", title, input, status });
    } else if (type === "image_generation_call") {
      const result = typeof item.result === "string" ? item.result : "";
      const imagePath = result
        ? result.startsWith("data:")
          ? result
          : `data:image/png;base64,${result}`
        : undefined;
      attachOutput(
        stored.id,
        typeof item.id === "string" ? item.id : stored.id,
        "image_generation",
        typeof item.revised_prompt === "string" ? item.revised_prompt : "",
        media ?? (imagePath ? [{ kind: "image", path: imagePath }] : undefined),
        resolveToolActivityStatus(
          typeof item.status === "string" ? item.status : "completed",
          "",
        ),
      );
    } else if (type === "function_call_output" || type === "custom_tool_call_output" || type === "tool_search_output") {
      const output = itemText(item);
      const persistedStatus =
        typeof metadata?.astro_tool_status === "string"
          ? metadata.astro_tool_status
          : typeof item.status === "string"
            ? item.status
            : "completed";
      attachOutput(
        stored.id,
        typeof item.call_id === "string" ? item.call_id : undefined,
        typeof item.name === "string"
          ? item.name
          : type === "tool_search_output"
            ? "tool_search"
            : undefined,
        output,
        media,
        resolveToolActivityStatus(persistedStatus, output),
      );
    }
  }
  return bubbles;
}

function sumReasoningDurations(
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

/** 同轮多段 assistant（工具循环）合并为一条气泡，取最完整 timeline。 */
export function coalesceConsecutiveAssistants(
  messages: HistoryBubble[],
): HistoryBubble[] {
  const out: HistoryBubble[] = [];
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
    prev.activities = mergeActivities(
      prev.activities ?? [],
      m.activities ?? [],
    );
    if (
      (m.segments?.length ?? 0) >= (prev.segments?.length ?? 0) &&
      m.segments?.length
    ) {
      prev.segments = m.segments;
    }
    if (
      (m.uiSurfaces?.length ?? 0) >= (prev.uiSurfaces?.length ?? 0) &&
      m.uiSurfaces?.length
    ) {
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
    } else if (type === "text") {
      out.push({
        type: "text",
        id: id || `txt-${out.length}`,
        text: typeof seg.text === "string" ? seg.text : "",
        at,
      });
    } else if (type === "activity") {
      const executionMode =
        seg.executionMode === "serial" || seg.executionMode === "parallel"
          ? seg.executionMode
          : seg.execution_mode === "serial" || seg.execution_mode === "parallel"
            ? seg.execution_mode
            : undefined;
      const batchId =
        typeof seg.batchId === "string"
          ? seg.batchId
          : typeof seg.batch_id === "string"
            ? seg.batch_id
            : undefined;
      out.push({
        type: "activity",
        id: id || `a-${out.length}`,
        at,
        batchId,
        executionMode,
      });
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
    const activitySegment =
      segments[idx]!.type === "activity" ? segments[idx]! : null;
    const next = segments[idx + 1];
    let durationSec = act.durationSec;
    if (next && next.at > at) {
      durationSec = Math.round(((next.at - at) / 1000) * 10) / 10;
    }
    return {
      ...act,
      at: act.at ?? at,
      durationSec: durationSec && durationSec > 0 ? durationSec : undefined,
      batchId: act.batchId ?? activitySegment?.batchId,
      executionMode: act.executionMode ?? activitySegment?.executionMode,
    };
  });
}

function mapActivity(a: ChatHistoryActivityDto): ChatActivity {
  const kind = ACTIVITY_KINDS.has(a.kind as ChatActivityKind)
    ? (a.kind as ChatActivityKind)
    : "tool";
  const status = ACTIVITY_STATUSES.has(a.status as ChatActivityStatus)
    ? (a.status as ChatActivityStatus)
    : undefined;
  const media = Array.isArray(a.media)
    ? (a.media
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
        .filter(Boolean) as NonNullable<ChatActivity["media"]>)
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
    if (
      !message.activities?.some((activity) =>
        isLiveActivityStatus(activity.status),
      )
    ) {
      return message;
    }
    return {
      ...message,
      activities: message.activities.map((activity) => {
        if (!isLiveActivityStatus(activity.status)) return activity;
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

export function normalizeHistoryBubbles(
  messages: HistoryBubble[],
): ChatMessage[] {
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

export function mapHistoryItems(items: StoredResponseItemDto[]): ChatMessage[] {
  return normalizeHistoryBubbles(responseItemsToBubbles(items));
}
