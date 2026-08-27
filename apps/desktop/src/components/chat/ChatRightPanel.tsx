// 聊天悬浮侧栏（摘要 / 上下文 / 分支）；文件预览在内嵌项目工作台。
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  GitBranch,
  LayoutDashboard,
  Layers,
  PanelRight,
  X,
  type LucideIcon,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import type { ChatMessage } from "../../types";
import type { ContextUsageSnapshot } from "../../lib/chat/contextUsage";
import {
  CHAT_RIGHT_PANEL_DEFAULT_WIDTH,
  CHAT_RIGHT_PANEL_MIN_WIDTH,
  CHAT_RIGHT_PANEL_WIDTH_KEY,
  clampChatRightPanelWidth,
  maxChatRightPanelWidth,
  parseStoredChatRightPanelWidth,
} from "../../lib/ui/chatRightPanelWidth";
import ContextExplorer from "./ContextExplorer";
import TaskMonitorPanel from "./TaskMonitorPanel";
import ChatAgentInfo from "./ChatAgentInfo";
import BranchGraphPanel from "./BranchGraphPanel";
import MotionSwitch from "../ui/MotionSwitch";
import SubagentActivityBar from "./SubagentActivityBar";
import SubagentsPanel from "./SubagentsPanel";
import type { AgentThread, AgentTreeNode } from "../../hooks/chat/subagentTree";

export type ChatRightTab = "summary" | "context" | "branches";

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
  /** 聊天消息列表（任务监控 Tab 使用） */
  messages?: ChatMessage[];
  /** 是否正在流式输出 */
  streaming?: boolean;
  subagentRoots: AgentTreeNode[];
  subagentThreads: AgentThread[];
  subagentError?: string | null;
  subagentsLoading: boolean;
  subagentsInitialized: boolean;
  onRefreshSubagents: () => Promise<void>;
  onMarkSubagentRead: (canonicalPath: string) => void;
  onOpenSession: (sessionId: string) => void | Promise<void>;
  onOpenSideSession: (sessionId: string) => void | Promise<void>;
  /** 在某轮之前分支后，把原始输入回填到输入框 */
  onPrefillInput?: (text: string) => void;
  onOpenMemory: () => void;
  onOpenSkills: () => void;
  onWidthChange?: (width: number) => void;
};

const TAB_KEYS: Record<ChatRightTab, MessageKey> = {
  summary: "chat.rightPanel.summary",
  context: "chat.rightPanel.context",
  branches: "chat.rightPanel.branches",
};

const TAB_ICONS: Record<ChatRightTab, LucideIcon> = {
  summary: LayoutDashboard,
  context: Layers,
  branches: GitBranch,
};

const RESIZE_KEYBOARD_STEP = 16;
const RESIZE_KEYBOARD_LARGE_STEP = 48;

export default function ChatRightPanel({
  tab,
  onTabChange,
  onClose,
  sessionId,
  turnId = null,
  tokenUsage: _tokenUsage = null,
  contextUsage = null,
  contextWindow = 0,
  messages = [],
  streaming = false,
  subagentRoots,
  subagentThreads,
  subagentError = null,
  subagentsLoading,
  subagentsInitialized,
  onRefreshSubagents,
  onMarkSubagentRead,
  onOpenSession,
  onOpenSideSession,
  onPrefillInput,
  onOpenMemory,
  onOpenSkills,
  onWidthChange,
}: Props) {
  const { t } = useI18n();
  const tabs: ChatRightTab[] = ["summary", "context", "branches"];
  const tabsRef = useRef<HTMLDivElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const panelWidthRef = useRef(CHAT_RIGHT_PANEL_DEFAULT_WIDTH);
  const resizeRef = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null);
  const [indicator, setIndicator] = useState({ left: 0, width: 0, ready: false });
  const [panelWidth, setPanelWidth] = useState(() => {
    try {
      return parseStoredChatRightPanelWidth(localStorage.getItem(CHAT_RIGHT_PANEL_WIDTH_KEY));
    } catch {
      return CHAT_RIGHT_PANEL_DEFAULT_WIDTH;
    }
  });
  const [maxPanelWidth, setMaxPanelWidth] = useState(CHAT_RIGHT_PANEL_DEFAULT_WIDTH);
  const [resizing, setResizing] = useState(false);
  const [subagentsOpen, setSubagentsOpen] = useState(false);
  const [selectedSubagentPath, setSelectedSubagentPath] = useState<string | null>(null);

  panelWidthRef.current = panelWidth;

  useEffect(() => {
    onWidthChange?.(panelWidth);
  }, [onWidthChange, panelWidth]);

  useEffect(() => {
    if (tab !== "summary") {
      setSubagentsOpen(false);
      setSelectedSubagentPath(null);
    }
  }, [tab]);

  const containerWidth = useCallback(() => {
    const width = panelRef.current?.parentElement?.getBoundingClientRect().width ?? 0;
    return width > 0 ? width : Number.POSITIVE_INFINITY;
  }, []);

  const updatePanelWidth = useCallback((nextWidth: number, persist = false) => {
    const next = clampChatRightPanelWidth(nextWidth, containerWidth());
    panelWidthRef.current = next;
    setPanelWidth(next);
    if (persist) {
      try {
        localStorage.setItem(CHAT_RIGHT_PANEL_WIDTH_KEY, String(next));
      } catch {
        // Storage can be unavailable in private or locked-down webviews.
      }
    }
  }, [containerWidth]);

  const closeWithAnim = useCallback(() => {
    const el = panelRef.current;
    if (!el || window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      onClose();
      return;
    }
    el.classList.add("is-closing");
    let done = false;
    const finish = () => { if (done) return; done = true; onClose(); };
    el.addEventListener("animationend", finish, { once: true });
    setTimeout(finish, 160);
  }, [onClose]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeWithAnim();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeWithAnim]);

  useLayoutEffect(() => {
    const container = panelRef.current?.parentElement;
    if (!container) return;

    const syncBounds = () => {
      const width = container.getBoundingClientRect().width;
      if (width <= 0) return;
      const nextMax = maxChatRightPanelWidth(width);
      setMaxPanelWidth(nextMax);
      const nextWidth = clampChatRightPanelWidth(panelWidthRef.current, width);
      panelWidthRef.current = nextWidth;
      setPanelWidth(nextWidth);
    };

    syncBounds();
    const observer = typeof ResizeObserver !== "undefined" ? new ResizeObserver(syncBounds) : null;
    observer?.observe(container);
    window.addEventListener("resize", syncBounds);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", syncBounds);
    };
  }, []);

  const finishResize = useCallback((pointerId: number) => {
    if (resizeRef.current?.pointerId !== pointerId) return;
    resizeRef.current = null;
    setResizing(false);
    try {
      localStorage.setItem(CHAT_RIGHT_PANEL_WIDTH_KEY, String(panelWidthRef.current));
    } catch {
      // Storage can be unavailable in private or locked-down webviews.
    }
  }, []);

  const onResizePointerDown = (event: ReactPointerEvent<HTMLButtonElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    resizeRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: panelRef.current?.getBoundingClientRect().width ?? panelWidthRef.current,
    };
    setResizing(true);
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const onResizePointerMove = (event: ReactPointerEvent<HTMLButtonElement>) => {
    const drag = resizeRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    updatePanelWidth(drag.startWidth + drag.startX - event.clientX);
  };

  const onResizePointerUp = (event: ReactPointerEvent<HTMLButtonElement>) => {
    finishResize(event.pointerId);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onResizeKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    const step = event.shiftKey ? RESIZE_KEYBOARD_LARGE_STEP : RESIZE_KEYBOARD_STEP;
    let nextWidth: number | null = null;
    if (event.key === "ArrowLeft") nextWidth = panelWidthRef.current + step;
    else if (event.key === "ArrowRight") nextWidth = panelWidthRef.current - step;
    else if (event.key === "Home") nextWidth = CHAT_RIGHT_PANEL_MIN_WIDTH;
    else if (event.key === "End") nextWidth = maxPanelWidth;
    if (nextWidth == null) return;
    event.preventDefault();
    updatePanelWidth(nextWidth, true);
  };

  useLayoutEffect(() => {
    const root = tabsRef.current;
    if (!root) return;

    const sync = () => {
      const active = root.querySelector<HTMLElement>(".chat-right-tab.is-active");
      if (!active) return;
      const inset = Math.round(active.offsetWidth * 0.16);
      setIndicator({
        left: active.offsetLeft + inset,
        width: Math.max(0, active.offsetWidth - inset * 2),
        ready: true,
      });
    };

    sync();
    const ro = typeof ResizeObserver !== "undefined" ? new ResizeObserver(sync) : null;
    ro?.observe(root);
    window.addEventListener("resize", sync);
    return () => {
      ro?.disconnect();
      window.removeEventListener("resize", sync);
    };
  }, [tab, t]);

  return (
    <>
      <button
        type="button"
        className="chat-right-backdrop"
        aria-label={t("chat.rightPanel.close")}
        onClick={closeWithAnim}
      />
      <aside
        ref={panelRef}
        className={`chat-right-panel${resizing ? " is-resizing" : ""}`}
        aria-label={t("chat.rightPanel.title")}
        style={{ "--chat-right-panel-width": `${panelWidth}px` } as CSSProperties}
      >
        <button
          type="button"
          className="chat-right-resizer"
          role="separator"
          aria-label={t("chat.rightPanel.resize")}
          aria-orientation="vertical"
          aria-valuemin={Math.min(CHAT_RIGHT_PANEL_MIN_WIDTH, maxPanelWidth)}
          aria-valuemax={maxPanelWidth}
          aria-valuenow={panelWidth}
          title={t("chat.rightPanel.resize")}
          onDoubleClick={() => updatePanelWidth(CHAT_RIGHT_PANEL_DEFAULT_WIDTH, true)}
          onKeyDown={onResizeKeyDown}
          onPointerDown={onResizePointerDown}
          onPointerMove={onResizePointerMove}
          onPointerUp={onResizePointerUp}
          onPointerCancel={(event) => finishResize(event.pointerId)}
          onLostPointerCapture={(event) => finishResize(event.pointerId)}
        />
        <div className="chat-right-header">
          <h2 className="chat-right-title">
            <PanelRight size={17} strokeWidth={1.75} aria-hidden />
            {t("chat.rightPanel.title")}
          </h2>
          <button
            type="button"
            className="chat-right-close"
            onClick={closeWithAnim}
            title={t("chat.rightPanel.close")}
            aria-label={t("chat.rightPanel.close")}
          >
            <X size={14} strokeWidth={1.75} aria-hidden />
          </button>
        </div>
        <div className="chat-right-tabs" role="tablist" ref={tabsRef}>
          <span
            className={`chat-right-tab-indicator${indicator.ready ? " is-ready" : ""}`}
            aria-hidden
            style={{
              transform: `translateX(${indicator.left}px)`,
              width: indicator.width,
            }}
          />
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
                <Icon size={15} strokeWidth={1.75} aria-hidden />
                {t(TAB_KEYS[id])}
              </button>
            );
          })}
        </div>
        <div className="chat-right-body">
          <MotionSwitch
            switchKey={tab}
            className="anim-switch--fill"
            variant="fade"
          >
            {tab === "summary" && (
              subagentsOpen ? (
                <SubagentsPanel
                  embedded
                  open
                  rootSessionId={sessionId}
                  roots={subagentRoots}
                  threads={subagentThreads}
                  initialTarget={selectedSubagentPath}
                  streamError={subagentError}
                  loading={subagentsLoading}
                  initialized={subagentsInitialized}
                  refreshThreads={onRefreshSubagents}
                  markRead={onMarkSubagentRead}
                  onClose={() => {
                    setSubagentsOpen(false);
                    setSelectedSubagentPath(null);
                  }}
                />
              ) : (
                <div className="chat-summary-panel">
                  <ChatAgentInfo
                    variant="summary"
                    sessionId={sessionId}
                    turnId={turnId}
                    contextUsage={contextUsage}
                    contextWindow={contextWindow}
                    onOpenMemory={onOpenMemory}
                    onOpenSkills={onOpenSkills}
                    onOpenContextTab={() => onTabChange("context")}
                  />
                  <SubagentActivityBar
                    rootSessionId={sessionId}
                    roots={subagentRoots}
                    onRefresh={onRefreshSubagents}
                    showEmpty
                    onOpenPanel={() => {
                      setSelectedSubagentPath(null);
                      setSubagentsOpen(true);
                    }}
                    onOpenThread={(canonicalPath) => {
                      onMarkSubagentRead(canonicalPath);
                      setSelectedSubagentPath(canonicalPath);
                      setSubagentsOpen(true);
                    }}
                  />
                  <TaskMonitorPanel messages={messages} streaming={streaming} />
                </div>
              )
            )}
            {tab === "context" && (
              <div className="chat-context-stack">
                <ContextExplorer
                  snapshot={contextUsage}
                  windowTokens={contextWindow}
                  sessionLabel={sessionId ?? "—"}
                />
                <ChatAgentInfo
                  variant="context"
                  sessionId={sessionId}
                  turnId={turnId}
                  contextUsage={contextUsage}
                  contextWindow={contextWindow}
                  onOpenMemory={onOpenMemory}
                  onOpenSkills={onOpenSkills}
                  onOpenContextTab={() => onTabChange("context")}
                />
              </div>
            )}
            {tab === "branches" && (
              <BranchGraphPanel
                sessionId={sessionId}
                streaming={streaming}
                onOpenSession={onOpenSession}
                onOpenSideSession={onOpenSideSession}
                onPrefillInput={onPrefillInput}
              />
            )}
          </MotionSwitch>
        </div>
      </aside>
    </>
  );
}
