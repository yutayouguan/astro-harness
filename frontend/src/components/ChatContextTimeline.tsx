/** 上下文时间线条目。 */
import { useMemo, useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import type { ChatActivity, ChatActivityKind, ChatMessage } from "../types";

/** 上下文时间线入参 */
type Props = { messages: ChatMessage[] };

const CONTEXT_KINDS = new Set<ChatActivityKind>([
  "tool",
  "skill",
  "mcp",
  "hook",
  "memory",
]);

const KIND_KEYS: Record<ChatActivityKind, MessageKey> = {
  tool: "chat.activity.kind.tool",
  skill: "chat.activity.kind.skill",
  mcp: "chat.activity.kind.mcp",
  hook: "chat.activity.kind.hook",
  memory: "chat.activity.kind.memory",
  status: "chat.activity.kind.status",
};

const STATUS_KEYS: Record<NonNullable<ChatActivity["status"]>, MessageKey> = {
  running: "chat.activity.status.running",
  done: "chat.activity.status.done",
  error: "chat.activity.status.error",
};

export default function ChatContextTimeline({ messages }: Props) {
  const { t } = useI18n();
  const [openId, setOpenId] = useState<string | null>(null);

  const activities = useMemo(() => {
    const out: ChatActivity[] = [];
    for (const m of messages) {
      for (const a of m.activities ?? []) {
        if (CONTEXT_KINDS.has(a.kind)) out.push(a);
      }
    }
    return out;
  }, [messages]);

  if (activities.length === 0) {
    return <p className="muted">{t("chat.rightPanel.noContext")}</p>;
  }

  return (
    <ul className="chat-context-timeline">
      {activities.map((a) => {
        const open = openId === a.id;
        return (
          <li key={a.id} className={a.status === "running" ? "is-running" : undefined}>
            <button
              type="button"
              className={`chat-context-item ${a.status === "running" ? "is-running" : ""}`}
              onClick={() => setOpenId(open ? null : a.id)}
              aria-expanded={open}
            >
              <span data-kind={a.kind}>{t(KIND_KEYS[a.kind])}</span>
              <strong>{a.title}</strong>
              {a.status ? (
                <span data-status={a.status}>{t(STATUS_KEYS[a.status])}</span>
              ) : null}
            </button>
            {open && a.detail ? (
              <pre className="chat-context-detail">{a.detail}</pre>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
