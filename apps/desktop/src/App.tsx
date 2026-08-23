/** 根布局：侧栏导航、聊天与各功能面板编排。 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import AboutDialog from "./components/ui/AboutDialog";
import ChatRightPanel from "./components/chat/ChatRightPanel";
import ProjectContextMenu from "./components/chat/ProjectContextMenu";
import ProjectEditDialog from "./components/chat/ProjectEditDialog";
import SidebarSessionList from "./components/chat/SidebarSessionList";
import ChatView from "./components/chat/ChatView";
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
  type ChatWorkMode,
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
  type SettingsTabId,
} from "./lib/ui/navConfig";
import {
  readFilesSubmode,
  writeFilesSubmode,
  type FilesSubmode,
} from "./lib/filespace/filesMode";
import {
  Brain,
  ChartPie,
  Cpu,
  FolderClosed,
  FolderOpen,
  Info,
  Layers2,
  MessageSquare,
  ScrollText,
  Settings2,
  Sparkles,
  Store,
  Wrench,
} from "lucide-react";
import { syncWindowUnderlay } from "./lib/ui/windowUnderlay";
import { dynamicGradientForTab } from "./lib/ui/dynamicGradient";
import {
  applyShellGradientVars,
  clearShellGradientVars,
  flushGlassBackdrop,
} from "./lib/ui/shellGradient";
import type {
  ModelCapabilities,
  ModelPricingMeta,
  ModelReasoningMeta,
  ProjectDto,
  ProviderModelsResult,
} from "./types";

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
  const [chatMode, setChatMode] = useState<ChatWorkMode>(() => loadChatMode());
  const onChatModeChange = useCallback((mode: ChatWorkMode) => {
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
  const [settingsTab, setSettingsTab] = useState<SettingsTabId>("preferences");
  const [projects, setProjects] = useState<ProjectDto[]>([]);
  const [activeProjectId, setActiveProjectId] = useState("default");
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(new Set());
  const [projectMenu, setProjectMenu] = useState<{ id: string; name: string; x: number; y: number } | null>(null);
  const [editingProject, setEditingProject] = useState<ProjectDto | null>(null);
  const DEFAULT_PROJECT: ProjectDto = {
    id: "default",
    name: "默认工作空间",
    roots: [],
    position: -1,
    createdAt: "",
    updatedAt: "",
  };
  // 启动时从后端加载项目列表，始终保证有默认工作空间
  useEffect(() => {
    void invoke<ProjectDto[]>("list_projects")
      .then((list) => {
        const loaded = list ?? [];
        if (!loaded.some((p) => p.id === "default")) {
          loaded.unshift(DEFAULT_PROJECT);
        }
        setProjects(loaded);
      })
      .catch(() => setProjects([DEFAULT_PROJECT]));
  }, []);
  /** 导航到 settings 并切换到指定子 tab */
  const openSettingsTab = useCallback((tab: SettingsTabId) => {
    setSettingsTab(tab);
    setNav("settings");
  }, []);
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
          openSettingsTab("tools");
          break;
        case "nav_skills":
          setNav("skills");
          break;
        case "nav_mcp":
          setToolsInitialTab("mcp");
          openSettingsTab("tools");
          break;
        case "nav_memory":
          openSettingsTab("memory");
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
          openSettingsTab("insights");
          break;
        case "nav_providers":
          openSettingsTab("providers");
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
      openSettingsTab,
    ],
  );

  // ── Layout helpers ────────────────────────────────────────────────────────
  const meta = PAGE_META["chat"];
  const ActiveIcon = IconChat;

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
          <div className="sidebar-projects">
            <div className="sidebar-section-header">
              <span className="sidebar-section-title">项目</span>
              <button
                type="button"
                className="sidebar-add-btn"
                onClick={async () => {
                  try {
                    const { open } = await import("@tauri-apps/plugin-dialog");
                    const selected = await open({ directory: true, title: "选择项目文件夹" });
                    if (selected && typeof selected === "string") {
                      const newProject: ProjectDto = {
                        id: crypto.randomUUID(),
                        name: selected.split("/").pop() || selected,
                        roots: [selected],
                        position: projects.length,
                        createdAt: new Date().toISOString(),
                        updatedAt: new Date().toISOString(),
                      };
                      setProjects((prev) => {
                        if (prev.some((p) => p.roots[0] === selected)) return prev;
                        return [...prev, newProject];
                      });
                      setActiveProjectId(newProject.id);
                    }
                  } catch {}
                }}
                title="+ 新建项目"
                aria-label="新建项目"
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                  <path d="M12 5v14" /><path d="M5 12h14" />
                </svg>
              </button>
            </div>
            {projects.map((proj) => (
              <div
                key={proj.id}
                className={`sidebar-project ${activeProjectId === proj.id ? "is-active" : ""}`}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setProjectMenu({ ...proj, x: e.clientX, y: e.clientY });
                }}
              >
                <div className="sidebar-project-header">
                  <button
                    type="button"
                    className="sidebar-project-name"
                    onClick={() => {
                      setCollapsedProjects((prev) => {
                        const next = new Set(prev);
                        if (next.has(proj.id)) next.delete(proj.id);
                        else next.add(proj.id);
                        return next;
                      });
                      setActiveProjectId(proj.id);
                      setNav("chat");
                    }}
                  >
                    {collapsedProjects.has(proj.id)
                      ? <FolderClosed size={16} strokeWidth={1.7} aria-hidden />
                      : <FolderOpen size={16} strokeWidth={1.7} aria-hidden />
                    }
                    <span className="sidebar-item-label">{proj.name}</span>
                  </button>
                  <button
                    type="button"
                    className="sidebar-project-more"
                    onClick={(e) => {
                      e.stopPropagation();
                      const rect = e.currentTarget.getBoundingClientRect();
                      setProjectMenu({ ...proj, x: rect.right + 4, y: rect.top });
                    }}
                    title="更多"
                    aria-label="更多"
                  >
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
                      <circle cx="12" cy="5" r="1.5" /><circle cx="12" cy="12" r="1.5" /><circle cx="12" cy="19" r="1.5" />
                    </svg>
                  </button>
                  <button
                    type="button"
                    className="sidebar-project-action"
                    onClick={() => { setActiveProjectId(proj.id); setNav("chat"); startNewChat(); }}
                    title="新建会话"
                    aria-label="新建会话"
                  >
                    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                      <path d="M12 20h9" /><path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z" />
                    </svg>
                  </button>
                </div>
                {!collapsedProjects.has(proj.id) && (
                  <SidebarSessionList
                    activeSessionId={chat.sessionId}
                    projectId={proj.id}
                    onOpenSession={(sid) => { setActiveProjectId(proj.id); void openSessionFromFilespace(sid); }}
                    onDeleteCurrentSession={() => { void prepareDeleteCurrentSession(); void clearDeletedCurrentSession(); }}
                  />
                )}
              </div>
            ))}
          </div>
          <button
            type="button"
            className="sidebar-settings-btn"
            onClick={() => setNav("settings")}
          >
            <Settings2 size={15} strokeWidth={1.8} aria-hidden />
            <span className="sidebar-item-label">{t("nav.settings")}</span>
            {chat.memoryPendingCount > 0 && (
              <span className="nav-badge">
                {chat.memoryPendingCount > 99 ? "99+" : String(chat.memoryPendingCount)}
              </span>
            )}
          </button>
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
              {showsHeaderAgentPicker(nav) && (
                <AgentPicker
                  className="header-agent-picker"
                  agents={agents}
                  value={activeAgentId}
                  onChange={(id) => {
                    void setActiveAgent(id).catch((e) => {
                      console.warn("set_active_agent failed", e);
                    });
                  }}
                  onCreateNew={startNewAgent}
                  menuAlign="end"
                />
              )}
              <ModelPicker
                providers={providers}
                value={activeProviderId}
                onChange={(id, model) => void onChatModelChange(id, model)}
                onActivePrefsChange={syncComposerFromModelPrefs}
                disabled={chat.streaming}
              />
              <div className="chat-header-tools">
                <button
                  type="button"
                  className="header-icon-btn"
                  onClick={() => { setNav("chat"); startNewChat(); }}
                  data-tip={t("sidebar.newChat")}
                  aria-label={t("sidebar.newChat")}
                >
                  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z" />
                    <path d="M12 8v8" /><path d="M8 12h8" />
                  </svg>
                </button>
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
            </div>
          </div>
          <div className="page-body">
                <div className="chat-layout-with-right">
                  <div className="chat-main">
                    <ChatView
                      sessionId={chat.sessionId}
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
                      onSteerQueuedFollowUp={chat.steerQueuedFollowUp}
                      onOpenQueuedFollowUpInNewTask={chat.openQueuedFollowUpInNewTask}
                      onCloseQueuedFollowUps={chat.closeQueuedFollowUps}
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
                        openSettingsTab("tools");
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
                      onOpenMemory={() => openSettingsTab("memory")}
                      onOpenSkills={() => setNav("skills")}
                    />
                  )}
                </div>
          </div>
        </section>
      </div>

      {/* ── Overlay panels ─────────────────────────────────────────────────── */}
      {nav === "settings" && (
        <div className="settings-overlay" onClick={(e) => { if (e.target === e.currentTarget) setNav("chat"); }}>
          <div className="settings-overlay-panel">
            <button type="button" className="settings-overlay-close" onClick={() => setNav("chat")} aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
            </button>
            {settingsTab === "memory" && memoryHeaderAgent?.show && (
              <div className="settings-overlay-agent-picker">
                <AgentPicker
                  className="header-agent-picker"
                  agents={agents}
                  value={memoryHeaderAgent.value}
                  onChange={memoryHeaderAgent.onChange}
                  allOption={memoryHeaderAgent.allOption}
                  labelKey="memory.agents"
                  menuAlign="end"
                />
              </div>
            )}
            <div className="settings-layout">
              <nav className="settings-sidebar" aria-label="Settings">
                {([
                  { id: "preferences", label: "通用", Icon: Settings2 },
                  { id: "preferences:appearance", label: "外观", Icon: Sparkles },
                  { id: "preferences:conversation", label: "对话", Icon: MessageSquare },
                  { id: "preferences:context", label: "上下文与压缩", Icon: Layers2 },
                  { id: "providers", label: "模型配置", Icon: Cpu },
                  { id: "tools", label: "工具", Icon: Wrench },
                  { id: "memory", label: "记忆", Icon: Brain },
                  { id: "models", label: "模型市场", Icon: Store },
                  { id: "insights", label: "洞察", Icon: ChartPie },
                  { id: "preferences:diagnostics", label: "诊断", Icon: ScrollText },
                  { id: "preferences:about", label: "关于", Icon: Info },
                ] as const).map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className={`settings-sidebar-item ${settingsTab === item.id ? "is-active" : ""}`}
                    onClick={() => setSettingsTab(item.id as SettingsTabId)}
                  >
                    <item.Icon size={18} strokeWidth={1.6} aria-hidden />
                    {item.label}
                  </button>
                ))}
              </nav>
              <div className="settings-content">
                {settingsTab.startsWith("preferences") && (
                  <PreferencesPanel
                    section={settingsTab === "preferences" ? "general" : settingsTab.split(":")[1] as import("./components/settings/PreferencesPanel").PreferenceCategory}
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
                {settingsTab === "tools" && (
                  <ToolsPanel
                    active={nav === "settings"}
                    initialTab={toolsInitialTab}
                    onInitialTabConsumed={() => setToolsInitialTab(null)}
                  />
                )}
                {settingsTab === "evolution" && (
                  <EvolutionModelsPanel active={nav === "settings"} tone={shellTone} />
                )}
                {settingsTab === "insights" && (
                  <InsightsPanel active={nav === "settings"} />
                )}
                {settingsTab === "models" && (
                  <ModelMarketPanel active={nav === "settings"} />
                )}
                {settingsTab === "providers" && (
                  <ProvidersPanel
                    active={nav === "settings"}
                    onStateChange={syncProvidersFromState}
                    tone={shellTone}
                  />
                )}
                {settingsTab === "memory" && (
                  <MemoryPanel
                    onClose={() => setSettingsTab("preferences")}
                    sessionId={chat.sessionId}
                    onHeaderAgentPickerChange={setMemoryHeaderAgent}
                  />
                )}
              </div>
            </div>
          </div>
        </div>
      )}

      {nav === "files" && (
        <div className="settings-overlay" onClick={(e) => { if (e.target === e.currentTarget) setNav("chat"); }}>
          <div className="settings-overlay-panel settings-overlay-panel--wide">
            <button type="button" className="settings-overlay-close" onClick={() => setNav("chat")} aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
            </button>
            <FilesPage
              active={nav === "files"}
              submode={filesMode}
              onSubmodeChange={changeFilesMode}
              onOpenSession={openSessionFromFilespace}
              onAttachFiles={attachArtifactsToChat}
              onClose={() => setNav("chat")}
            />
          </div>
        </div>
      )}

      {nav === "skills" && (
        <div className="settings-overlay" onClick={(e) => { if (e.target === e.currentTarget) setNav("chat"); }}>
          <div className="settings-overlay-panel">
            <button type="button" className="settings-overlay-close" onClick={() => setNav("chat")} aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
            </button>
            <SkillsPanel
              active={nav === "skills"}
              onInstallWithAgent={(prompt) => {
                setInput(prompt);
                setNav("chat");
              }}
              tone={shellTone}
            />
          </div>
        </div>
      )}

      {nav === "loop" && (
        <div className="settings-overlay" onClick={(e) => { if (e.target === e.currentTarget) setNav("chat"); }}>
          <div className="settings-overlay-panel settings-overlay-panel--wide">
            <button type="button" className="settings-overlay-close" onClick={() => setNav("chat")} aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>
            </button>
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
          </div>
        </div>
      )}

      {projectMenu && (
        <ProjectContextMenu
          x={projectMenu.x}
          y={projectMenu.y}
          projectName={projectMenu.name}
          projectPath={projects.find((p) => p.id === projectMenu.id)?.roots[0] ?? ""}
          onAction={(action) => {
            if (action === "remove") {
              void invoke("delete_project", { projectId: projectMenu.id }).catch(() => {});
              setProjects((prev) => prev.filter((p) => p.id !== projectMenu.id));
              if (activeProjectId === projectMenu.id) {
                setActiveProjectId(projects[0]?.id ?? "default");
              }
            } else if (action === "reveal") {
              const root = projects.find((p) => p.id === projectMenu.id)?.roots[0];
              if (root) {
                void import("@tauri-apps/plugin-shell").then((mod) =>
                  mod.open(root)
                ).catch(() => {});
              }
            } else if (action === "pin") {
              void invoke("move_project", { projectId: projectMenu.id, beforeProjectId: null }).then(() =>
                invoke<ProjectDto[]>("list_projects").then((list) => {
                  if (list) setProjects(list);
                }),
              ).catch(() => {});
            } else if (action === "edit") {
              const proj = projects.find((p) => p.id === projectMenu.id);
              if (proj) setEditingProject(proj);
            } else if (action === "worktree") {
              window.alert("功能开发中");
            } else if (action === "archive") {
              void invoke<import("./types").RecentSessionDto[]>("list_sessions", {
                filter: "active",
                limit: 200,
                projectId: projectMenu.id,
              }).then((sessions) => {
                if (sessions) {
                  for (const s of sessions) {
                    void invoke("archive_session", { sessionId: s.sessionId }).catch(() => {});
                  }
                }
              }).catch(() => {});
            }
          }}
          onClose={() => setProjectMenu(null)}
        />
      )}
      {toastHost}
      <AboutDialog open={aboutOpen} onClose={() => setAboutOpen(false)} />
      <ProjectEditDialog
        open={editingProject !== null}
        project={editingProject}
        onClose={() => setEditingProject(null)}
        onUpdated={(updated) => {
          setProjects((prev) => prev.map((p) => (p.id === updated.id ? updated : p)));
        }}
        onRemoved={(id) => {
          setProjects((prev) => prev.filter((p) => p.id !== id));
          if (activeProjectId === id) {
            setActiveProjectId(projects[0]?.id ?? "default");
          }
        }}
      />
    </div>
  );
}
