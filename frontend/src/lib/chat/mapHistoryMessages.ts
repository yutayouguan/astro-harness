/** 将 `get_chat_history` DTO 映射为 UI `ChatMessage[]`。 */
import type {
  ChatActivity,
  ChatActivityKind,
  ChatHistoryMessageDto,
  ChatMessage,
} from "../../types";

const ACTIVITY_KINDS = new Set<ChatActivityKind>([
  "tool",
  "skill",
  "mcp",
  "hook",
  "memory",
  "status",
]);

export function mapHistoryMessages(messages: ChatHistoryMessageDto[]): ChatMessage[] {
  return messages
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => {
      const activities: ChatActivity[] | undefined =
        m.activities && m.activities.length > 0
          ? m.activities.map((a) => {
              const kind = ACTIVITY_KINDS.has(a.kind as ChatActivityKind)
                ? (a.kind as ChatActivityKind)
                : "tool";
              const status =
                a.status === "running" || a.status === "done" || a.status === "error"
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
            })
          : undefined;
      const segments =
        Array.isArray(m.segments) && m.segments.length > 0 ? m.segments : undefined;
      const uiSurfaces =
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
      return {
        id: m.id,
        role: m.role as "user" | "assistant",
        content: m.content,
        reasoning: m.reasoning ?? undefined,
        activities,
        segments,
        uiSurfaces,
      };
    });
}
