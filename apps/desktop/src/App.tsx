/** 根布局：侧栏导航、聊天与各功能面板编排。 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import AnimatedSwitch from "./components/ui/AnimatedSwitch";
import AboutDialog from "./components/ui/AboutDialog";
import ChatRightPanel from "./components/chat/ChatRightPanel";
import ChatView from "./components/chat/ChatView";
import CronPanel from "./components/schedule/CronPanel";
import LoopPanel from "./components/loop/LoopPanel";
import FilesPage from "./components/files/FilesPage";
import InsightsPanel from "./components/settings/InsightsPanel";
import ModelMarketPanel from "./components/settings/ModelMarketPanel";
import MemoryPanel, {
  type MemoryHeaderAgentPicker,
} from "./components/settings/MemoryPanel";
import AgentPicker from "./components/agents/AgentPicker";
import ModelPicker from "./components/agents/ModelPicker";
import PreferencesPanel from "./components/settings/PreferencesPanel";
import ProvidersPanel from "./components/settings/ProvidersPanel";
import SidebarContextMenu from "./components/settings/SidebarContextMenu";
import SkillsPanel from "./components/settings/SkillsPanel";
import ToolsPanel from "./components/settings/ToolsPanel";
import EvolutionModelsPanel from "./components/settings/EvolutionModelsPanel";
import {
  AstroLogoMark,
  IconChat,
  IconPanelClose,
  IconPanelOpen,
  IconRightPanel,
  IconSidebarIcons,
  IconSidebarLabels,
} from "./components/icons";
import { useActiveAgent } from "./hooks/app/useActiveAgent";
import { useChatDisplayPrefs } from "./hooks/chat/useChatDisplayPrefs";
import { useChatSession } from "./hooks/chat/useChatSession";
import { useChatThinkingPrefs } from "./hooks/chat/useChatThinkingPrefs";
import { useBeautifyTips } from "./hooks/ui/useBeautifyTips";
import { useProviders } from "./hooks/providers/useProviders";
import { useShellColorStyle } from "./hooks/app/useShellColorStyle";
import { useSidebar } from "./hooks/app/useSidebar";
import { useTheme } from "./hooks/app/useTheme";
import { useTransientToast } from "./hooks/ui/useTransientToast";
import { useWindowChrome } from "./hooks/app/useWindowChrome";
import { useI18n } from "./i18n/LocaleContext";
import type { MessageKey } from "./i18n/messages";
import {
  defaultThinkingLevelFromMeta,
  thinkingLevelsFromMeta,
  type ThinkingLevel,
} from "./lib/chat/thinkingPrefs";
import {
  modelPrefsToThinkingLevel,
  hasSavedModelPrefs,
  loadPickerGlobals,
  loadModelPrefs,
  prefsFromReasoningMeta,
  syncMaxModeWithThinkingLevel,
  thinkingLevelToModelPatch,
  upsertModelPrefs,
  type ModelPickerGlobals,
  type ModelRuntimePrefs,
} from "./lib/model/modelPrefs";
import { shouldShowThinkingControls } from "./lib/chat/shouldShowThinkingControls";
import {
  CHAT_MODES,
  loadChatMode,
  saveChatMode,
  type ChatInteractionMode,
} from "./lib/chat/chatMode";
import type { SlashAction } from "./lib/chat/composerCommands";
import {
  resolveContextWindow,
  usagePercent,
} from "./lib/chat/contextUsage";
import {
  NAV,
  PAGE_META,
  showsHeaderAgentPicker,
  type NavId,
} from "./lib/ui/navConfig";
import {
  readFilesSubmode,
  writeFilesSubmode,
  type FilesSubmode,
} from "./lib/filespace/filesMode";
import { FolderTree, Sparkles } from "lucide-react";
import { syncWindowUnderlay } from "./lib/ui/windowUnderlay";
import {
  dynamicGradientForTab,
  toneCssVarsFromHex,
} from "./lib/ui/dynamicGradient";
import {
  applyShellGradientVars,
  clearShellGradientVars,
  flushGlassBackdrop,
} from "./lib/ui/shellGradient";
import type {
  ModelCapabilities,
  ModelPricingMeta,
  ModelReasoningMeta,
  ProviderModelsResult,
} from "./types";

const NAV_ROW_PITCH_PX = 44;
export default function App() {
  // ── Theme / i18n / prefs ──────────────────────────────────────────────────
  const { mode, setMode, resolved, reassert } = useTheme();
  const {
    colorStyle,
    gradient,
    dynamicSeed,
    setColorStyle,
    setGradient,
    reshuffleDynamic,
    beginGradientEdit,
    previewGradient,
    commitGradientEdit,
    cancelGradientEdit,
  } = useShellColorStyle();
  useBeautifyTips();
  const { t, locale } = useI18n();
  const { prefs: chatDisplayPrefs, setVerbosity, setToggle } = useChatDisplayPrefs();
  const chatDisplayPrefsRef = useRef(chatDisplayPrefs);
  chatDisplayPrefsRef.current = chatDisplayPrefs;
  const { thinkingPrefs, setLevel: setThinkingLevel } = useChatThinkingPrefs();
  const { showToast: showTransientToast, toastHost } = useTransientToast();

  // ── App-level state ───────────────────────────────────────────────────────
  const [chatMode, setChatMode] = useState<ChatInteractionMode>(() => loadChatMode());
  const onChatModeChange = useCallback((mode: ChatInteractionMode) => {
    setChatMode(mode);
    saveChatMode(mode);
  }, []);
  const syncComposerFromModelPrefs = useCallback(
    (prefs: ModelRuntimePrefs, globals: ModelPickerGlobals) => {
      setThinkingLevel(modelPrefsToThinkingLevel(prefs, globals));
    },
    [setThinkingLevel],
  );
  const [nav, setNav] = useState<NavId>(NAV[0].id);
  const [filesMode, setFilesMode] = useState<FilesSubmode>(() =>
    readFilesSubmode(),
  );
  const changeFilesMode = (mode: FilesSubmode) => {
    setFilesMode(mode);
    writeFilesSubmode(mode);
  };
  const [toolsInitialTab, setToolsInitialTab] = useState<"builtin" | "mcp" | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [modelContextWindow, setModelContextWindow] = useState<number | null>(null);
  const [activeModelCapabilities, setActiveModelCapabilities] =
    useState<ModelCapabilities | null>(null);
  const [activeModelReasoning, setActiveModelReasoning] =
    useState<ModelReasoningMeta | null>(null);
  const [activeModelPricing, setActiveModelPricing] =
    useState<ModelPricingMeta | null>(null);
  const [memoryHeaderAgent, setMemoryHeaderAgent] =
    useState<MemoryHeaderAgentPicker | null>(null);

  // ── Extracted hooks ───────────────────────────────────────────────────────
  const sidebar = useSidebar();
  const winChrome = useWindowChrome();
  const {
    agents,
    activeAgentId,
    setActiveAgent,
  } = useActiveAgent();
  const {
    providers,
    activeProviderId,
    activeProvider,
    onChatModelChange,
    syncProvidersFromState,
  } = useProviders();
  const chat = useChatSession({
    activeProvider,
    providers,
    chatMode,
    onChatModeChange,
    chatDisplayPrefsRef,
    locale,
    t,
    showTransientToast,
    nav,
    setNav,
  });
  const {
    send,
    startNewChat,
    startNewAgent,
    skipAgentCreate,
    runCompactSession,
    undoLastExchange,
    retryLastAssistant,
    stopStream,
    pauseStream,
    resumeStream,
    regenerateMessage,
    editUserMessage,
    deleteMessage,
    branchMessage,
    onUiAction,
    openSessionFromFilespace,
    attachArtifactsToChat,
    setChatRightOpen,
    setChatRightTab,
    setMemoryPendingCount,
    setInput,
    setFocusMessageId,
    prepareDeleteCurrentSession,
    clearDeletedCurrentSession,
  } = chat;

  // ── Model context window + capabilities ───────────────────────────────────
  useEffect(() => {
    const providerId = activeProvider?.id;
    const modelId = activeProvider?.model;
    if (!providerId || !modelId) {
      setModelContextWindow(null);
      setActiveModelCapabilities(null);
      setActiveModelReasoning(null);
      setActiveModelPricing(null);
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const cached = await invoke<ProviderModelsResult | null>(
          "get_cached_provider_models",
          { id: providerId },
        );
        if (cancelled) return;
        const match = cached?.models?.find((m) => m.id === modelId);
        const win = match?.context_window;
        setModelContextWindow(typeof win === "number" && win > 0 ? win : null);
        setActiveModelCapabilities(match?.capabilities ?? null);
        setActiveModelPricing(match?.pricing ?? null);
        const reasoning = match?.reasoning ?? null;
        setActiveModelReasoning(reasoning);
        // 无本地偏好时，按 OpenRouter default_enabled / default_effort 播种
        if (reasoning && !hasSavedModelPrefs(providerId, modelId)) {
          const seeded = prefsFromReasoningMeta(reasoning);
          upsertModelPrefs(providerId, modelId, seeded);
          syncComposerFromModelPrefs(seeded, loadPickerGlobals());
        } else {
          const prefs = loadModelPrefs(providerId, modelId);
          const globals = loadPickerGlobals();
          const level = modelPrefsToThinkingLevel(prefs, globals);
          const allowed = thinkingLevelsFromMeta(reasoning);
          if (!allowed.includes(level)) {
            const snapped = defaultThinkingLevelFromMeta(reasoning);
            const next = {
              ...prefs,
              ...thinkingLevelToModelPatch(snapped),
            };
            upsertModelPrefs(providerId, modelId, next);
            syncComposerFromModelPrefs(next, globals);
          } else {
            syncComposerFromModelPrefs(prefs, globals);
          }
        }
      } catch {
        if (!cancelled) {
          setModelContextWindow(null);
          setActiveModelCapabilities(null);
          setActiveModelReasoning(null);
          setActiveModelPricing(null);
        }
      }
    })();
    return () => { cancelled = true; };
  }, [activeProvider?.id, activeProvider?.model, syncComposerFromModelPrefs]);

  // ── ⌘/Ctrl+N：聊天页新建会话 ─────────────────────────────────────────────
  useEffect(() => {
    if (nav !== "chat") return;
    const onKey = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || e.shiftKey) return;
      if (e.key.toLowerCase() !== "n") return;
      if (e.isComposing) return;
      e.preventDefault();
      void startNewChat();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [nav, startNewChat]);

  // ── macOS open-preferences / open-about listener ─────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    let unlistenPrefs: (() => void) | undefined;
    let unlistenAbout: (() => void) | undefined;
    void listen("open-preferences", () => {
      setNav("settings");
    })
      .then((fn) => {
        unlistenPrefs = fn;
      })
      .catch(() => {});
    void listen("open-about", () => {
      setAboutOpen(true);
    })
      .then((fn) => {
        unlistenAbout = fn;
      })
      .catch(() => {});
    return () => {
      unlistenPrefs?.();
      unlistenAbout?.();
    };
  }, []);

  // ── Nav tone + underlay ───────────────────────────────────────────────────
  const activeTone = NAV.find((n) => n.id === nav)?.tone ?? "blue";
  /** 统一/灵动用 custom gradient；多彩跟随当前 tab 的 tone token */
  const usesShellGradient = colorStyle === "unified" || colorStyle === "dynamic";
  const shellTone = usesShellGradient ? "blue" : activeTone;
  const activeShellGradient = useMemo(() => {
    if (colorStyle === "unified") return gradient;
    if (colorStyle === "dynamic") {
      return dynamicGradientForTab(dynamicSeed, nav, resolved);
    }
    return null;
  }, [colorStyle, gradient, dynamicSeed, nav, resolved]);
  // ── Tone crossfade overlay ─────────────────────────────────────────────
  const prevToneRef = useRef(shellTone);
  const [toneFadeBg, setToneFadeBg] = useState<string | null>(null);
  const shellRef = useRef<HTMLDivElement | null>(null);

  // layout：在绘制前写 CSS 变量，并刷新玻璃层（避免 backdrop-filter 缓存旧色）
  useLayoutEffect(() => {
    const root = document.documentElement;
    if (prevToneRef.current !== shellTone && colorStyle === "colorful" && shellRef.current) {
      const bg = getComputedStyle(shellRef.current).background;
      if (bg) setToneFadeBg(bg);
    }
    prevToneRef.current = shellTone;
    root.setAttribute("data-tone", shellTone);
    root.setAttribute("data-color-style", colorStyle);
    if (activeShellGradient) {
      applyShellGradientVars(root, activeShellGradient, resolved);
      flushGlassBackdrop(root);
    } else {
      clearShellGradientVars(root);
    }
    reassert();
  }, [shellTone, colorStyle, activeShellGradient, resolved, reassert]);
  useEffect(() => {
    void syncWindowUnderlay(resolved, shellTone, activeShellGradient);
  }, [resolved, shellTone, activeShellGradient]);

  // ── Derived ───────────────────────────────────────────────────────────────
  const contextWindow = resolveContextWindow(
    modelContextWindow,
    chat.contextUsage?.contextWindow,
  );
  const showThinking = shouldShowThinkingControls({
    capabilities: activeModelCapabilities,
    backendId: activeProvider?.backend_id,
  });
  const statusText = chat.statusDetail ?? t(`status.${chat.statusPhase}` as MessageKey);
  // ── Thinking callbacks ────────────────────────────────────────────────────
  const onThinkingLevelChange = useCallback(
    (level: ThinkingLevel) => {
      setThinkingLevel(level);
      syncMaxModeWithThinkingLevel(level);
      if (!activeProvider) return;
      upsertModelPrefs(activeProvider.id, activeProvider.model, thinkingLevelToModelPatch(level));
    },
    [activeProvider, setThinkingLevel],
  );
  const onToggleThinking = useCallback(() => {
    const next: ThinkingLevel = thinkingPrefs.level === "off" ? "high" : "off";
    onThinkingLevelChange(next);
  }, [thinkingPrefs.level, onThinkingLevelChange]);

  // ── handleSlashAction (cross-cutting: chat + nav + prefs) ─────────────────
  const handleSlashAction = useCallback(
    (action: SlashAction, _args?: string) => {
      switch (action) {
        case "new_chat":
          startNewChat();
          break;
        case "compact":
          void runCompactSession();
          break;
        case "undo":
          undoLastExchange();
          break;
        case "retry":
          retryLastAssistant();
          break;
        case "stop":
          void stopStream();
          break;
        case "status": {
          // 只用后端 context_usage 快照；无快照或未知窗口时不编造百分比
          const ctxLabel =
            chat.contextUsage && contextWindow > 0
              ? `${usagePercent(chat.contextUsage.totalTokens, contextWindow)}%`
              : chat.contextUsage
                ? `~${chat.contextUsage.totalTokens}`
                : "—";
          showTransientToast(
            t("chat.slashStatusMsg", {
              session: chat.sessionId ? chat.sessionId.slice(0, 8) : "—",
              provider: activeProvider?.display_name ?? "—",
              model: activeProvider?.model ?? "—",
              mode: chatMode,
              thinking: thinkingPrefs.level,
              verbosity: chatDisplayPrefs.verbosity,
              ctx: ctxLabel,
            }),
          );
          break;
        }
        case "usage":
          if (!chat.tokenUsage || chat.tokenUsage.totalTokens <= 0) {
            showTransientToast(t("chat.slashUsageEmpty"));
          } else {
            showTransientToast(
              t("chat.slashUsageMsg", {
                total: String(chat.tokenUsage.totalTokens),
                prompt: String(chat.tokenUsage.promptTokens),
                completion: String(chat.tokenUsage.completionTokens),
              }),
            );
          }
          break;
        case "model":
          showTransientToast(
            t("chat.slashModelMsg", {
              provider: activeProvider?.display_name ?? "—",
              model: activeProvider?.model ?? "—",
            }),
          );
          break;
        case "verbose": {
          const order = ["compact", "normal", "detailed"] as const;
          const i = order.indexOf(chatDisplayPrefs.verbosity);
          const next = order[(i + 1) % order.length];
          setVerbosity(next);
          showTransientToast(t("chat.slashVerboseMsg", { level: next }));
          break;
        }
        case "reasoning": {
          const order = thinkingLevelsFromMeta(activeModelReasoning);
          const i = Math.max(0, order.indexOf(thinkingPrefs.level));
          const next = order[(i + 1) % order.length] ?? "high";
          onThinkingLevelChange(next);
          showTransientToast(t("chat.slashReasoningMsg", { level: next }));
          break;
        }
        case "mode": {
          const i = CHAT_MODES.indexOf(chatMode);
          const next = CHAT_MODES[(i + 1) % CHAT_MODES.length];
          onChatModeChange(next);
          showTransientToast(t("chat.slashModeMsg", { mode: next }));
          break;
        }
        case "nav_tools":
          setToolsInitialTab("builtin");
          setNav("tools");
          break;
        case "nav_skills":
          setNav("skills");
          break;
        case "nav_mcp":
          setToolsInitialTab("mcp");
          setNav("tools");
          break;
        case "nav_memory":
          setNav("memory");
          break;
        case "memory_list": {
          void (async () => {
            try {
              const rows = await invoke<{ id: string; action: string; target: string; source: string }[]>(
                "list_pending_memory_writes",
              );
              if (!rows?.length) {
                showTransientToast(t("memory.pending.emptyTitle"));
                setMemoryPendingCount(0);
                return;
              }
              setMemoryPendingCount(rows.length);
              const lines = rows
                .slice(0, 5)
                .map((r) => `${r.id.slice(0, 8)} ${r.action}/${r.target} (${r.source})`);
              const more = rows.length > 5 ? ` …+${rows.length - 5}` : "";
              showTransientToast(`${lines.join(" · ")}${more}`);
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_approve": {
          void (async () => {
            try {
              const id = (_args ?? "").trim();
              const msg =
                !id || id === "all"
                  ? await invoke<string>("approve_all_pending_memory_writes")
                  : await invoke<string>("approve_pending_memory_write", { id });
              showTransientToast(msg || t("memory.pending.approved"));
              if (chat.sessionId) {
                try {
                  const settings = await invoke<{ autoRefreshOnUpdate: boolean }>("get_memory_settings");
                  if (settings.autoRefreshOnUpdate !== false) {
                    await invoke("refresh_memory", { agentId: null, sessionId: chat.sessionId });
                  }
                } catch {
                  // ignore refresh errors
                }
              }
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_reject": {
          void (async () => {
            try {
              const id = (_args ?? "").trim();
              if (!id || id === "all") {
                const msg = await invoke<string>("reject_all_pending_memory_writes");
                showTransientToast(msg);
              } else {
                await invoke("reject_pending_memory_write", { id });
                showTransientToast(t("memory.pending.rejected"));
              }
            } catch (e) {
              showTransientToast(String(e));
            }
          })();
          break;
        }
        case "memory_refresh": {
          void (async () => {
            try {
              await invoke("refresh_memory", { agentId: null, sessionId: chat.sessionId ?? null });
              showTransientToast(t("memory.refresh.done"), { tone: "success" });
            } catch (e) {
              showTransientToast(String(e), { tone: "error" });
            }
          })();
          break;
        }
        case "memory_help":
          showTransientToast(t("memory.slash.help"));
          break;
        case "nav_insights":
          setNav("insights");
          break;
        case "nav_providers":
          setNav("providers");
          break;
        case "nav_settings":
          setNav("settings");
          break;
        case "open_context":
          setChatRightTab("context");
          setChatRightOpen(true);
          setNav("chat");
          break;
        default:
          break;
      }
    },
    [
      startNewChat,
      runCompactSession,
      undoLastExchange,
      retryLastAssistant,
      stopStream,
      chat.tokenUsage,
      chat.contextUsage,
      chat.sessionId,
      contextWindow,
      activeProvider,
      chatMode,
      thinkingPrefs.level,
      activeModelReasoning,
      chatDisplayPrefs.verbosity,
      showTransientToast,
      t,
      setVerbosity,
      setThinkingLevel,
      onThinkingLevelChange,
      onChatModeChange,
      setMemoryPendingCount,
      setChatRightTab,
      setChatRightOpen,
    ],
  );

  // ── Layout helpers ────────────────────────────────────────────────────────
  const meta = PAGE_META[nav];
  const ActiveIcon = NAV.find((n) => n.id === nav)?.Icon ?? IconChat;
  const activeNavIndex = NAV.findIndex((item) => item.id === nav);
  const sidebarNavStyle = {
    "--nav-indicator-y": `${Math.max(activeNavIndex, 0) * NAV_ROW_PITCH_PX}px`,
  } as CSSProperties;

  // ── JSX ───────────────────────────────────────────────────────────────────
  return (
    <div
      ref={shellRef}
      className={`app-shell ${winChrome.windowMaximized ? "is-maximized" : ""}`}
      data-tone={shellTone}
      data-color-style={colorStyle}
    >
      {toneFadeBg && (
        <div
          className="shell-tone-crossfade"
          style={{ background: toneFadeBg }}
          onAnimationEnd={() => setToneFadeBg(null)}
          aria-hidden
        />
      )}
      <div
        className="native-drag-region"
        onMouseDown={(e) => void winChrome.onTitleMouseDown(e)}
        onDoubleClick={(e) => void winChrome.onTitleDoubleClick(e)}
        aria-hidden
      />

      <div className="titlebar-sidebar-toggle">
        <button
          type="button"
          className="sidebar-pin-btn"
          data-tone={shellTone}
          onClick={sidebar.toggleSidebar}
          title={sidebar.sidebarPinned ? t("sidebar.unpin") : t("sidebar.pin")}
          aria-label={sidebar.sidebarPinned ? t("sidebar.unpinAria") : t("sidebar.pinAria")}
          aria-pressed={sidebar.sidebarPinned}
        >
          {sidebar.sidebarPinned ? (
            <IconPanelClose width={13} height={13} />
          ) : (
            <IconPanelOpen width={13} height={13} />
          )}
        </button>
        <button
          type="button"
          className="sidebar-new-chat-btn"
          data-tone={shellTone}
          onClick={() => { setNav("chat"); startNewChat(); }}
          title={t("sidebar.newChat" as never)}
          aria-label={t("sidebar.newChat" as never)}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z" />
            <path d="M12 8v8" /><path d="M8 12h8" />
          </svg>
        </button>
      </div>

      {!sidebar.sidebarPinned && (
        <div
          className="sidebar-hotzone"
          onMouseEnter={sidebar.openSidebar}
          aria-hidden
        />
      )}

      <div className="body-row">
        <aside
          className={`sidebar ${sidebar.sidebarOpen || sidebar.sidebarPinned ? "is-open" : "is-collapsed"} ${sidebar.sidebarPinned ? "is-pinned" : ""} ${sidebar.showSidebarLabels ? "is-labels" : "is-icons"}`}
          onMouseEnter={sidebar.openSidebar}
          onMouseLeave={sidebar.scheduleHideSidebar}
          onContextMenu={sidebar.openSidebarContextMenu}
        >
          <div className="sidebar-brand">
            <div className="sidebar-logo" aria-hidden>
              <AstroLogoMark width={26} height={26} />
            </div>
            <div className="sidebar-brand-text">Astro Agent</div>
            <div className="sidebar-brand-actions">
              <button
                type="button"
                className="sidebar-pin-btn"
                data-tone={shellTone}
                onClick={sidebar.toggleSidebarLabels}
                title={sidebar.sidebarLabels ? t("sidebar.hideLabels") : t("sidebar.showLabels")}
                aria-label={
                  sidebar.sidebarLabels ? t("sidebar.hideLabelsAria") : t("sidebar.showLabelsAria")
                }
                aria-pressed={sidebar.sidebarLabels}
              >
                {sidebar.sidebarLabels ? (
                  <IconSidebarIcons width={15} height={15} />
                ) : (
                  <IconSidebarLabels width={15} height={15} />
                )}
              </button>
            </div>
          </div>
          <div className="sidebar-nav" style={sidebarNavStyle}>
            <span className="sidebar-nav-indicator" aria-hidden />
            {NAV.map((item) => {
              const label = t(item.labelKey);
              const pendingBadge =
                item.id === "memory" && chat.memoryPendingCount > 0
                  ? chat.memoryPendingCount > 99
                    ? "99+"
                    : String(chat.memoryPendingCount)
                  : null;
              const dynamicToneStyle =
                colorStyle === "dynamic"
                  ? (toneCssVarsFromHex(
                      dynamicGradientForTab(dynamicSeed, item.id, resolved)
                        .primary.color,
                    ) as CSSProperties)
                  : undefined;
              return (
                <button
                  key={item.id}
                  className={`nav-item ${nav === item.id ? "active" : ""}`}
                  data-tone={usesShellGradient ? shellTone : item.tone}
                  style={dynamicToneStyle}
                  onClick={() => setNav(item.id)}
                  {...(sidebar.showSidebarLabels
                    ? {}
                    : { "data-tip": label, "data-tip-pos": "right" as const })}
                  aria-label={pendingBadge ? `${label} (${pendingBadge})` : label}
                >
                  <span className="nav-icon" aria-hidden>
                    <item.Icon />
                    {pendingBadge ? <span className="nav-badge">{pendingBadge}</span> : null}
                  </span>
                  <span className="nav-label">{label}</span>
                </button>
              );
            })}
          </div>
        </aside>
        {sidebar.sidebarCtx ? (
          <SidebarContextMenu
            x={sidebar.sidebarCtx.x}
            y={sidebar.sidebarCtx.y}
            labelsVisible={sidebar.sidebarLabels}
            pinned={sidebar.sidebarPinned}
            onAction={sidebar.onSidebarContextAction}
            onClose={sidebar.closeSidebarContextMenu}
          />
        ) : null}

        <section className="content-pane">
          <div className="content-header">
            <div className="content-heading">
              <div className="page-title-block">
                <div className="page-title-icon" data-tone={shellTone} aria-hidden>
                  <ActiveIcon width={15} height={15} />
                </div>
                <div className="page-title-text">
                  <h1 className="content-title" data-tone={shellTone}>
                    <span className="content-title-main">{t(meta.titleKey)}</span>
                    <span className="content-sub-sep" aria-hidden>
                      ·
                    </span>
                    <span className="content-sub">{t(meta.subKey)}</span>
                  </h1>
                </div>
              </div>
            </div>
            <div className="header-actions">
              <span className="status-chip">
                <span className={`status-dot ${chat.status}`} />
                {activeProvider?.display_name ?? t("status.none")} · {statusText}
              </span>
              {nav === "memory"
                ? memoryHeaderAgent?.show && (
                    <AgentPicker
                      className="header-agent-picker"
                      agents={agents}
                      value={memoryHeaderAgent.value}
                      onChange={memoryHeaderAgent.onChange}
                      allOption={memoryHeaderAgent.allOption}
                      labelKey="memory.agents"
                      menuAlign="end"
                    />
                  )
                : showsHeaderAgentPicker(nav) && (
                    <AgentPicker
                      className="header-agent-picker"
                      agents={agents}
                      value={activeAgentId}
                      onChange={(id) => {
                        void setActiveAgent(id).catch((e) => {
                          console.warn("set_active_agent failed", e);
                        });
                      }}
                      onCreateNew={nav === "chat" ? startNewAgent : undefined}
                      menuAlign="end"
                    />
                  )}
              {nav === "files" && (
                <div
                  className="files-mode-switch"
                  role="tablist"
                  aria-label={t("files.mode")}
                >
                  <button
                    type="button"
                    role="tab"
                    aria-selected={filesMode === "browse"}
                    className={`files-mode-tab ${filesMode === "browse" ? "is-active" : ""}`}
                    onClick={() => changeFilesMode("browse")}
                  >
                    <FolderTree size={15} strokeWidth={2.2} aria-hidden />
                    {t("files.mode.browse")}
                  </button>
                  <button
                    type="button"
                    role="tab"
                    aria-selected={filesMode === "artifacts"}
                    className={`files-mode-tab ${filesMode === "artifacts" ? "is-active" : ""}`}
                    onClick={() => changeFilesMode("artifacts")}
                  >
                    <Sparkles size={15} strokeWidth={2.2} aria-hidden />
                    {t("files.mode.artifacts")}
                  </button>
                </div>
              )}
              {nav === "chat" && (
                <ModelPicker
                  providers={providers}
                  value={activeProviderId}
                  onChange={(id, model) => void onChatModelChange(id, model)}
                  onActivePrefsChange={syncComposerFromModelPrefs}
                  disabled={chat.streaming}
                />
              )}
              {nav === "chat" && (
                <div className="chat-header-tools">
                  <button
                    type="button"
                    className={`header-icon-btn ${chat.chatRightOpen ? "is-active" : ""}`}
                    onClick={() => setChatRightOpen((open) => !open)}
                    title={t("chat.rightPanel.toggle")}
                    aria-label={t("chat.rightPanel.toggle")}
                    aria-pressed={chat.chatRightOpen}
                  >
                    <IconRightPanel width={16} height={16} />
                  </button>
                </div>
              )}
            </div>
          </div>
          <div className="page-body">
            <AnimatedSwitch switchKey={nav} className="anim-switch--fill">
              {nav === "chat" && (
                <div className="chat-layout-with-right">
                  <div className="chat-main">
                    <ChatView
                      messages={chat.messages}
                      input={chat.input}
                      attachments={chat.attachments}
                      streaming={chat.streaming}
                      turnInFlight={chat.turnInFlight}
                      streamPaused={chat.streamPaused}
                      sendBlocked={chat.isCompacting || chat.sessionReadOnly}
                      sendBlockedReason={
                        chat.isCompacting
                          ? t("chat.compactInProgress")
                          : chat.sessionReadOnly
                            ? chat.sessionEndReason === "compacted" || !chat.sessionEndReason
                              ? t("chat.sessionCompactedReadOnly")
                              : t("chat.sessionEndedReadOnly")
                            : undefined
                      }
                      displayPrefs={chatDisplayPrefs}
                      emptyMode={chat.emptyMode}
                      focusMessageId={chat.focusMessageId}
                      onFocusConsumed={() => setFocusMessageId(null)}
                      onInputChange={setInput}
                      onAttachmentsChange={chat.setAttachments}
                      onSend={send}
                      queuedFollowUps={chat.queuedFollowUps}
                      onRemoveQueuedFollowUp={chat.removeQueuedFollowUp}
                      onUpdateQueuedFollowUpText={chat.updateQueuedFollowUpText}
                      onMoveQueuedFollowUp={chat.moveQueuedFollowUp}
                      modeSwitchPrompt={chat.modeSwitchPrompt}
                      onApproveModeSwitch={chat.approveModeSwitch}
                      onDismissModeSwitch={chat.dismissModeSwitch}
                      parallelTasks={chat.parallelTasks}
                      onCancelParallelTask={chat.cancelParallelTask}
                      onWriteParallelSummary={chat.writeParallelSummary}
                      onClearSettledParallel={chat.clearSettledParallel}
                      pendingInterrupts={chat.sessionPendingInterrupts}
                      onUiAction={onUiAction}
                      onPauseStream={pauseStream}
                      onResumeStream={resumeStream}
                      onStopStream={stopStream}
                      onNewChat={startNewChat}
                      onSkipAgentCreate={skipAgentCreate}
                      onPickWelcomePrompt={(prompt) => setInput(prompt)}
                      showThinkingControls={showThinking}
                      reasoningMeta={activeModelReasoning}
                      thinkingPrefs={thinkingPrefs}
                      onToggleThinking={onToggleThinking}
                      onThinkingLevelChange={onThinkingLevelChange}
                      onOpenMcpSettings={() => {
                        setToolsInitialTab("mcp");
                        setNav("tools");
                      }}
                      chatMode={chatMode}
                      onChatModeChange={onChatModeChange}
                      onOpenContext={() => {
                        setChatRightTab("context");
                        setChatRightOpen(true);
                      }}
                      onRegenerateMessage={regenerateMessage}
                      onEditUserMessage={editUserMessage}
                      dissolvingIds={chat.dissolvingIds}
                      onDeleteMessage={deleteMessage}
                      onBranchMessage={(id) => void branchMessage(id)}
                      onSlashAction={handleSlashAction}
                      contextUsage={chat.contextUsage}
                      contextWindow={contextWindow}
                      modelId={activeProvider?.model ?? null}
                      modelCapabilities={activeModelCapabilities}
                      modelPricing={activeModelPricing}
                      contextUsagePercent={
                        chat.contextUsage && contextWindow > 0
                          ? usagePercent(chat.contextUsage.totalTokens, contextWindow)
                          : null
                      }
                    />
                  </div>
                  {chat.chatRightOpen && (
                    <ChatRightPanel
                      tab={chat.chatRightTab}
                      onTabChange={setChatRightTab}
                      onClose={() => setChatRightOpen(false)}
                      sessionId={chat.sessionId}
                      turnId={chat.currentTurnId}
                      tokenUsage={chat.tokenUsage}
                      contextUsage={chat.contextUsage}
                      contextWindow={contextWindow}
                      generatingPreview={chat.generatingPreview}
                      streamingSessionId={chat.streaming ? chat.sessionId : null}
                      onOpenSession={(id) => void openSessionFromFilespace(id)}
                      onNewSession={startNewChat}
                      onNewAgent={startNewAgent}
                      onPrepareDeleteCurrentSession={prepareDeleteCurrentSession}
                      onClearDeletedCurrentSession={clearDeletedCurrentSession}
                      onOpenMemory={() => setNav("memory")}
                      onOpenSkills={() => setNav("skills")}
                    />
                  )}
                </div>
              )}
              {nav === "memory" && (
                <MemoryPanel
                  onClose={() => setNav("chat")}
                  sessionId={chat.sessionId}
                  onHeaderAgentPickerChange={setMemoryHeaderAgent}
                />
              )}
              {nav === "files" && (
                <FilesPage
                  active={nav === "files"}
                  submode={filesMode}
                  onSubmodeChange={changeFilesMode}
                  onOpenSession={openSessionFromFilespace}
                  onAttachFiles={attachArtifactsToChat}
                  onClose={() => setNav("chat")}
                />
              )}
              {nav === "skills" && (
                <SkillsPanel
                  active={nav === "skills"}
                  onInstallWithAgent={(prompt) => {
                    setInput(prompt);
                    setNav("chat");
                  }}
                  tone={shellTone}
                />
              )}
              {nav === "settings" && (
                <PreferencesPanel
                  mode={mode}
                  onChange={setMode}
                  colorStyle={colorStyle}
                  onColorStyleChange={setColorStyle}
                  gradient={gradient}
                  onGradientChange={setGradient}
                  onBeginCustomGradient={beginGradientEdit}
                  onPreviewGradient={previewGradient}
                  onCommitCustomGradient={commitGradientEdit}
                  onCancelCustomGradient={cancelGradientEdit}
                  onReshuffleDynamic={reshuffleDynamic}
                  tone={shellTone}
                  chatDisplayPrefs={chatDisplayPrefs}
                  onChatVerbosityChange={setVerbosity}
                  onChatToggleChange={setToggle}
                  activeSessionId={chat.sessionId ?? undefined}
                />
              )}
              {nav === "tools" && (
                <ToolsPanel
                  active={nav === "tools"}
                  initialTab={toolsInitialTab}
                  onInitialTabConsumed={() => setToolsInitialTab(null)}
                />
              )}
              {nav === "evolution" && (
                <EvolutionModelsPanel active={nav === "evolution"} tone={shellTone} />
              )}
              {nav === "insights" && (
                <InsightsPanel active={nav === "insights"} />
              )}
              {nav === "models" && (
                <ModelMarketPanel active={nav === "models"} />
              )}
              {nav === "loop" && (
                <LoopPanel
                  active={nav === "loop"}
                  providers={providers.map((p) => ({
                    id: p.id,
                    name: p.display_name,
                    model: p.model,
                    kind: p.kind,
                  }))}
                  onCollapseSidebar={sidebar.collapseSidebar}
                  onExpandSidebar={sidebar.pinSidebar}
                />
              )}
              {nav === "cron" && (
                <CronPanel
                  active={nav === "cron"}
                  providers={providers.map((p) => ({
                    id: p.id,
                    name: p.display_name,
                    model: p.model,
                    kind: p.kind,
                  }))}
                  activeProviderId={activeProviderId}
                  tone={shellTone}
                />
              )}
              {nav === "providers" && (
                <ProvidersPanel
                  active={nav === "providers"}
                  onStateChange={syncProvidersFromState}
                  tone={shellTone}
                />
              )}
            </AnimatedSwitch>
          </div>
        </section>
      </div>
      {toastHost}
      <AboutDialog open={aboutOpen} onClose={() => setAboutOpen(false)} />
    </div>
  );
}
