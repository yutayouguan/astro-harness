import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Loader2, MessageSquare, SendHorizontal, Square, X } from "lucide-react";
import { motion, useReducedMotion } from "framer-motion";
import { useI18n } from "../../i18n/LocaleContext";
import type { ChatHistoryDto, ProviderDto } from "../../types";
import { ChatMarkdown } from "./ChatMarkdown";

type SideMessage = {
  id: string;
  role: "user" | "assistant";
  content: string;
  reasoning?: string;
  tools?: string[];
  error?: boolean;
};

type StreamPayload = {
  type: string;
  content?: string;
  message?: string;
  name?: string;
  outcome_type?: string;
};

/** 空态里的快捷追问，点击即发送。 */
const SUGGESTION_KEYS = [
  "chat.side.askProgress",
  "chat.side.askWhy",
  "chat.side.askRisk",
] as const;

type Props = {
  sessionId: string;
  provider: ProviderDto;
  interactionMode: string;
  onClose: () => void | Promise<void>;
};

/** Codex-style 旁路对话：独立流、独立输入框，不改变左侧主任务状态。 */
export default function SideChatPanel({
  sessionId,
  provider,
  interactionMode,
  onClose,
}: Props) {
  const { t } = useI18n();
  const reducedMotion = useReducedMotion();
  const [messages, setMessages] = useState<SideMessage[]>([]);
  const [input, setInput] = useState("");
  const [streaming, setStreaming] = useState(false);
  const [activeAssistantId, setActiveAssistantId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLTextAreaElement | null>(null);

  useEffect(() => {
    void invoke<ChatHistoryDto>("get_chat_history", {
      sessionId,
      limit: 120,
    }).then((history) => {
      setMessages(
        (history.messages ?? []).flatMap((message) =>
          message.role === "user" || message.role === "assistant"
            ? [{
                id: message.id,
                role: message.role,
                content: message.content,
                reasoning: message.reasoning ?? undefined,
              }]
            : [],
        ),
      );
    }).catch((reason) => setError(String(reason)));
  }, [sessionId]);

  useEffect(() => {
    scrollRef.current?.scrollTo({
      top: scrollRef.current.scrollHeight,
      behavior: "smooth",
    });
  }, [messages]);

  // 输入框跟随内容增高，超过上限后交给自身滚动。
  useEffect(() => {
    const field = inputRef.current;
    if (!field) return;
    field.style.height = "auto";
    field.style.height = `${Math.min(field.scrollHeight, 148)}px`;
  }, [input]);

  useEffect(
    () => () => {
      unlistenRef.current?.();
      unlistenRef.current = null;
    },
    [],
  );

  const stop = useCallback(async () => {
    if (!streaming) return;
    await invoke("chat_control", { sessionId, action: "cancel" }).catch(() => {});
  }, [sessionId, streaming]);

  const send = useCallback(async (prompt?: string) => {
    const content = (prompt ?? input).trim();
    if (!content || streaming) return;
    const userId = `side-u-${crypto.randomUUID()}`;
    const assistantId = `side-a-${crypto.randomUUID()}`;
    setInput("");
    setError(null);
    setStreaming(true);
    setActiveAssistantId(assistantId);
    setMessages((current) => [
      ...current,
      { id: userId, role: "user", content },
      { id: assistantId, role: "assistant", content: "" },
    ]);

    try {
      unlistenRef.current?.();
      const eventName = `chat_stream_${sessionId}`;
      unlistenRef.current = await listen<StreamPayload>(eventName, (event) => {
        const payload = event.payload;
        if (payload.type === "token" && payload.content) {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantId
                ? { ...message, content: message.content + payload.content }
                : message,
            ),
          );
        } else if (payload.type === "text_reconcile") {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantId
                ? { ...message, content: payload.content ?? message.content }
                : message,
            ),
          );
        } else if (payload.type === "reasoning" && payload.content) {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantId
                ? {
                    ...message,
                    reasoning: `${message.reasoning ?? ""}${payload.content}`,
                  }
                : message,
            ),
          );
        } else if (payload.type === "tool_call" && payload.name) {
          setMessages((current) =>
            current.map((message) =>
              message.id === assistantId
                ? {
                    ...message,
                    tools: Array.from(new Set([...(message.tools ?? []), payload.name!])),
                  }
                : message,
            ),
          );
        } else if (payload.type === "error") {
          const message = payload.message || t("status.unknownError");
          setError(message);
          setMessages((current) =>
            current.map((item) =>
              item.id === assistantId ? { ...item, error: true } : item,
            ),
          );
        } else if (payload.type === "done") {
          setStreaming(false);
          setActiveAssistantId(null);
          unlistenRef.current?.();
          unlistenRef.current = null;
        }
      });

      await invoke<string>("start_chat", {
        request: {
          content,
          provider: provider.backend_id,
          providerId: provider.id,
          model: provider.model,
          sessionId,
          useMemory: true,
          thinkingEnabled: false,
          reasoningEffort: "high",
          interactionMode,
          projectRoot: null,
          attachments: [],
        },
      });
    } catch (reason) {
      const message = String(reason);
      setError(message);
      setStreaming(false);
      setActiveAssistantId(null);
      unlistenRef.current?.();
      unlistenRef.current = null;
      setMessages((current) =>
        current.map((item) =>
          item.id === assistantId ? { ...item, content: message, error: true } : item,
        ),
      );
    }
  }, [input, interactionMode, provider, sessionId, streaming, t]);

  return (
    <motion.aside
      className="side-chat-panel"
      aria-label={t("chat.side.panel")}
      initial={reducedMotion ? { opacity: 0 } : { opacity: 0, x: 12, scale: 0.98 }}
      animate={{ opacity: 1, x: 0, scale: 1 }}
      exit={
        reducedMotion
          ? { opacity: 0, transition: { duration: 0.12 } }
          : {
              opacity: 0,
              x: 12,
              scale: 0.985,
              transition: { duration: 0.16, ease: "easeOut" },
            }
      }
      transition={{
        duration: reducedMotion ? 0.12 : 0.24,
        ease: [0.22, 1, 0.36, 1],
      }}
    >
      <header className="side-chat-head">
        <span className="side-chat-mark" aria-hidden>
          <MessageSquare size={15} />
        </span>
        <div className="side-chat-title">
          <strong>{t("chat.side.title")}</strong>
          <span title={t("chat.side.subtitle")}>{t("chat.side.subtitle")}</span>
        </div>
        <button
          type="button"
          className="side-chat-close"
          onClick={() => void onClose()}
          title={t("chat.side.close")}
          aria-label={t("chat.side.close")}
        >
          <X size={15} aria-hidden />
        </button>
      </header>

      <div className="side-chat-messages" ref={scrollRef}>
        {messages.length === 0 && (
          <div className="side-chat-empty">
            <span className="side-chat-empty-mark" aria-hidden>
              <MessageSquare size={22} />
            </span>
            <strong>{t("chat.side.emptyTitle")}</strong>
            <p>{t("chat.side.emptyHint")}</p>
            <div className="side-chat-suggestions">
              {SUGGESTION_KEYS.map((key) => (
                <button key={key} type="button" onClick={() => void send(t(key))}>
                  {t(key)}
                </button>
              ))}
            </div>
          </div>
        )}
        {messages.map((message) => (
          <article
            key={message.id}
            className={`side-chat-msg is-${message.role}${message.error ? " is-error" : ""}`}
          >
            {message.reasoning && (
              <details className="side-chat-reasoning">
                <summary>{t("chat.side.reasoning")}</summary>
                <p>{message.reasoning}</p>
              </details>
            )}
            {message.tools && message.tools.length > 0 && (
              <div className="side-chat-tools">
                {message.tools.map((tool) => <span key={tool}>{tool}</span>)}
              </div>
            )}
            <ChatMarkdown
              content={message.content}
              streaming={streaming && message.id === activeAssistantId}
              compact
              caret={streaming && message.id === activeAssistantId}
            />
          </article>
        ))}
        {error && <div className="side-chat-error">{error}</div>}
      </div>

      <footer className="side-chat-foot">
        <div className="side-chat-composer">
          <textarea
            ref={inputRef}
            value={input}
            onChange={(event) => setInput(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
                event.preventDefault();
                void send();
              }
            }}
            placeholder={t("chat.side.placeholder")}
            rows={1}
          />
          <button
            type="button"
            className={`side-chat-send${streaming ? " is-stop" : ""}`}
            onClick={() => void (streaming ? stop() : send())}
            disabled={!streaming && !input.trim()}
            title={streaming ? t("chat.side.stop") : t("chat.send")}
            aria-label={streaming ? t("chat.side.stop") : t("chat.send")}
          >
            {streaming
              ? <Square size={12} strokeWidth={2.4} fill="currentColor" aria-hidden />
              : <SendHorizontal size={15} strokeWidth={2.2} aria-hidden />}
          </button>
        </div>
        <div className="side-chat-hint">
          {streaming ? (
            <>
              <Loader2 className="side-chat-spinner" size={11} aria-hidden />
              {t("chat.side.thinking")}
            </>
          ) : (
            t("chat.side.enterHint")
          )}
        </div>
      </footer>
    </motion.aside>
  );
}
