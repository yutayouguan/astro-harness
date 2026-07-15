/** 聊天右侧栏（会话 / 上下文等 Tab）。 */
import { useEffect } from "react";
import {
  Bot,
  Layers,
  MessagesSquare,
  PanelRight,
  X,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import type { ContextUsageSnapshot } from "../lib/contextUsage";
import ChatSessionList from "./ChatSessionList";
import ChatAgentInfo from "./ChatAgentInfo";
import ContextExplorer from "./ContextExplorer";
import AnimatedSwitch from "./AnimatedSwitch";

/** 聊天右侧栏 Tab */
export type ChatRightTab = "sessions" | "context" | "agent";

/** 右侧栏入参 */
type Props = {
  tab: ChatRightTab;
  onTabChange: (t: ChatRightTab) => void;
  onClose: () => void;
  sessionId: string | null;
  turnId?: string | null;
  /** 当前会话累计 token（可选展示） */
  tokenUsage?: {
    promptTokens: number;
    completionTokens: number;
    totalTokens: number;
  } | null;
  /** 分层上下文占用快照（Context Explorer） */
  contextUsage?: ContextUsageSnapshot | null;
  /** 模型上下文窗口；未知时回落 128000 */
  contextWindow?: number;
  onOpenSession: (sessionId: string) => void;
  /** 新建空白会话 */
  onNewSession: () => void;
  onOpenMemory: () => void;
  onOpenSkills: () => void;
};

const TAB_KEYS: Record<ChatRightTab, MessageKey> = {
  sessions: "chat.rightPanel.sessions",
  context: "chat.rightPanel.context",
  agent: "chat.rightPanel.agent",
};

const TAB_ICONS: Record<ChatRightTab, LucideIcon> = {
  sessions: MessagesSquare,
  context: Layers,
  agent: Bot,
};

export default function ChatRightPanel({
  tab,
  onTabChange,
  onClose,
  sessionId,
  turnId = null,
  tokenUsage: _tokenUsage = null,
  contextUsage = null,
  contextWindow = 128_000,
  onOpenSession,
  onNewSession,
  onOpenMemory,
  onOpenSkills,
}: Props) {
  const { t } = useI18n();
  const tabs: ChatRightTab[] = ["sessions", "context", "agent"];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <button
        type="button"
        className="chat-right-backdrop"
        aria-label={t("chat.rightPanel.close")}
        onClick={onClose}
      />
      <aside className="chat-right-panel" aria-label={t("chat.rightPanel.title")}>
        <div className="chat-right-header">
          <h2 className="chat-right-title">
            <PanelRight size={15} strokeWidth={2.2} aria-hidden />
            {t("chat.rightPanel.title")}
          </h2>
          <button
            type="button"
            className="chat-right-close"
            onClick={onClose}
            title={t("chat.rightPanel.close")}
            aria-label={t("chat.rightPanel.close")}
          >
            <X size={14} strokeWidth={2.2} aria-hidden />
          </button>
        </div>
        <div className="chat-right-tabs" role="tablist">
          {tabs.map((id) => {
            const Icon = TAB_ICONS[id];
            return (
              <button
                key={id}
                type="button"
                role="tab"
                className={`chat-right-tab ${tab === id ? "is-active" : ""}`}
                aria-selected={tab === id}
                onClick={() => onTabChange(id)}
              >
                <Icon size={14} strokeWidth={2.2} aria-hidden />
                {t(TAB_KEYS[id])}
              </button>
            );
          })}
        </div>
        <div className="chat-right-body">
          <AnimatedSwitch switchKey={tab} className="anim-switch--fill" variant="fade">
            {tab === "sessions" && (
              <ChatSessionList
                activeSessionId={sessionId}
                onOpenSession={onOpenSession}
                onNewSession={onNewSession}
              />
            )}
            {tab === "context" && (
              <ContextExplorer
                snapshot={contextUsage}
                windowTokens={contextWindow}
                sessionLabel={sessionId ?? "—"}
              />
            )}
            {tab === "agent" && (
              <ChatAgentInfo
                sessionId={sessionId}
                turnId={turnId}
                onOpenMemory={onOpenMemory}
                onOpenSkills={onOpenSkills}
                onOpenContextTab={() => onTabChange("context")}
              />
            )}
          </AnimatedSwitch>
        </div>
      </aside>
    </>
  );
}
