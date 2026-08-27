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

import AboutDialog from "./components/ui/AboutDialog";
import ChatRightPanel, { type ChatRightTab } from "./components/chat/ChatRightPanel";
import SideChatPanel from "./components/chat/SideChatPanel";
import ProjectContextMenu from "./components/chat/ProjectContextMenu";
import ProjectEditDialog from "./components/chat/ProjectEditDialog";
import ProjectFolderIcon from "./components/chat/ProjectFolderIcon";
import SidebarSessionList from "./components/chat/SidebarSessionList";
import ChatView from "./components/chat/ChatView";
import ProjectFileEditor from "./components/chat/ProjectFileEditor";
import ProjectFilesPanel from "./components/chat/ProjectFilesPanel";
import LoopPanel from "./components/loop/LoopPanel";
import CronPanel from "./components/schedule/CronPanel";
import InsightsPanel from "./components/settings/InsightsPanel";
import ModelMarketPanel from "./components/settings/ModelMarketPanel";
import MemoryPanel from "./components/settings/MemoryPanel";
import ModelPicker from "./components/agents/ModelPicker";
import ExpandableSearch from "./components/ui/ExpandableSearch";
import PreferencesPanel from "./components/settings/PreferencesPanel";
import ProvidersPanel from "./components/settings/ProvidersPanel";
import SidebarContextMenu from "./components/settings/SidebarContextMenu";
import PluginsPage from "./components/plugins/PluginsPage";
import ToolsPanel from "./components/settings/ToolsPanel";
import EvolutionModelsPanel from "./components/settings/EvolutionModelsPanel";
import {
  AstroLogoMark,
  IconChat,
  IconCron,
  IconLoop,
  IconNewChat,
  IconPanelClose,
  IconPanelOpen,
  IconPlugin,
  IconRightPanel,
} from "./components/icons";
import { useChatDisplayPrefs } from "./hooks/chat/useChatDisplayPrefs";
import { useActiveSessionTitle } from "./hooks/chat/useActiveSessionTitle";
import { useChatSession } from "./hooks/chat/useChatSession";
import { useProjectFileWorkbench } from "./hooks/chat/useProjectFileWorkbench";
import { useSessionStatusMap } from "./hooks/chat/useSessionStatusMap";
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
import type { SessionListKind } from "./lib/chat/sessionManagement";
import {
  resolveContextWindow,
  usagePercent,
} from "./lib/chat/contextUsage";
import {
  NAV,
  PAGE_META,
  type NavId,
  type SettingsTabId,
} from "./lib/ui/navConfig";
import { SETTINGS_TABS, settingsTabMeta } from "./lib/ui/settingsTabs";
import { CHAT_RIGHT_PANEL_DEFAULT_WIDTH } from "./lib/ui/chatRightPanelWidth";
import {
  chatRightDockWidth,
  resolveChatRightDock,
} from "./lib/ui/chatRightDock";
import {
  ArrowLeft,
  Archive,
  ChevronRight,
  FolderTree,
  MessageSquare,
  MoreHorizontal,
  Settings2,
  Sparkles,
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

const ACTIVE_PROJECT_KEY = "astro.activeProjectId";

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
  const { t } = useI18n();
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
  // 选中的项目决定工具执行目录，重启后必须沿用上次的选择，否则会退回默认空间。
  const [activeProjectId, setActiveProjectId] = useState(
    () => localStorage.getItem(ACTIVE_PROJECT_KEY) ?? "default",
  );
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(new Set());
  const [collapsedSections, setCollapsedSections] = useState<Set<string>>(new Set());
  const [pinnedCount, setPinnedCount] = useState(0);
  const toggleSection = useCallback((section: string) => {
    setCollapsedSections((prev) => {
      const next = new Set(prev);
      if (next.has(section)) next.delete(section);
      else next.add(section);
      return next;
    });
  }, []);
  const [projectMenu, setProjectMenu] = useState<{ id: string; name: string; x: number; y: number } | null>(null);
  const [projectDialog, setProjectDialog] = useState<
    { mode: "create" } | { mode: "edit"; project: ProjectDto } | null
  >(null);
  const [conversationMenuOpen, setConversationMenuOpen] = useState(false);
  const conversationMenuRef = useRef<HTMLDivElement | null>(null);
  // 侧栏会话检索：搜索与归档视图跨全部项目生效
  const [sessionQuery, setSessionQuery] = useState("");
  const [sessionListKind, setSessionListKind] = useState<SessionListKind>("active");
  const searchingSessions = sessionQuery.trim().length > 0;
  // 启动时确保默认项目存在于 DB，然后加载全部项目
  useEffect(() => {
    void (async () => {
      try {
        await invoke<ProjectDto>("ensure_default_project");
        const list = await invoke<ProjectDto[]>("list_projects");
        setProjects(list ?? []);
        if (list?.length && !list.some((p) => p.id === activeProjectId)) {
          setActiveProjectId(list[0].id);
        }
      } catch {
        setProjects([]);
      }
    })();
  }, []);
  useEffect(() => {
    localStorage.setItem(ACTIVE_PROJECT_KEY, activeProjectId);
  }, [activeProjectId]);
  /** 导航到 settings 并切换到指定子 tab */
  const openSettingsTab = useCallback((tab: SettingsTabId) => {
    setSettingsTab(tab);
    setNav("settings");
  }, []);
  const [toolsInitialTab, setToolsInitialTab] = useState<"builtin" | null>(null);
  const [skillsInitialTab, setSkillsInitialTab] = useState<"mcp" | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [modelContextWindow, setModelContextWindow] = useState<number | null>(null);
  const [activeModelCapabilities, setActiveModelCapabilities] =
    useState<ModelCapabilities | null>(null);
  const [activeModelReasoning, setActiveModelReasoning] =
    useState<ModelReasoningMeta | null>(null);
  const [activeModelPricing, setActiveModelPricing] =
    useState<ModelPricingMeta | null>(null);
  // ── Extracted hooks ───────────────────────────────────────────────────────
  const sidebar = useSidebar();
  const winChrome = useWindowChrome();
  const {
    providers,
    activeProviderId,
    activeProvider,
    onChatModelChange,
    syncProvidersFromState,
  } = useProviders();
  const chat = useChatSession({
    activeProjectId,
    activeProvider,
    providers,
    chatMode,
    onChatModeChange,
    chatDisplayPrefsRef,
    t,
    showTransientToast,
    nav,
    setNav,
  });
  const sessionStatuses = useSessionStatusMap();
  const {
    send,
    startNewChat,
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
    setChatRightOpen,
    setChatRightTab,
    setMemoryPendingCount,
    setInput,
    setFocusMessageId,
    prepareDeleteCurrentSession,
    clearDeletedCurrentSession,
  } = chat;
  const activeProject = useMemo(
    () => projects.find((project) => project.id === activeProjectId) ?? null,
    [activeProjectId, projects],
  );
  const projectFiles = useProjectFileWorkbench(activeProject, chat.generatingPreview);
  const [projectFilesWidth, setProjectFilesWidth] = useState(264);
  const [chatRightPanelWidth, setChatRightPanelWidth] = useState(
    CHAT_RIGHT_PANEL_DEFAULT_WIDTH,
  );
  const [sideSessionId, setSideSessionId] = useState<string | null>(null);
  const [sideHostSessionId, setSideHostSessionId] = useState<string | null>(null);

  const closeSideChat = useCallback(async () => {
    const sideId = sideSessionId;
    setSideSessionId(null);
    setSideHostSessionId(null);
    if (!sideId) return;
    await invoke("discard_side_session", { sessionId: sideId }).catch((error) => {
      console.warn("discard_side_session failed", error);
    });
  }, [sideSessionId]);

  const openChatRightDock = useCallback(
    (tab?: ChatRightTab) => {
      projectFiles.setPanelOpen(false);
      if (sideSessionId) void closeSideChat();
      if (tab) setChatRightTab(tab);
      setChatRightOpen(true);
    },
    [
      closeSideChat,
      projectFiles.setPanelOpen,
      setChatRightOpen,
      setChatRightTab,
      sideSessionId,
    ],
  );

  const toggleChatRightDock = useCallback(() => {
    if (chat.chatRightOpen) {
      setChatRightOpen(false);
      return;
    }
    openChatRightDock();
  }, [chat.chatRightOpen, openChatRightDock, setChatRightOpen]);

  const toggleProjectFilesDock = useCallback(() => {
    if (projectFiles.panelOpen) {
      projectFiles.setPanelOpen(false);
      return;
    }
    setChatRightOpen(false);
    if (sideSessionId) void closeSideChat();
    projectFiles.setPanelOpen(true);
  }, [
    closeSideChat,
    projectFiles.panelOpen,
    projectFiles.setPanelOpen,
    setChatRightOpen,
    sideSessionId,
  ]);

  const startSideChat = useCallback(async () => {
    if (!chat.sessionId || !activeProvider || chat.streaming || sideSessionId) return;
    const keepChatBubbles = chat.messages.filter(
      (message) => message.id !== "welcome",
    ).length;
    if (keepChatBubbles === 0) return;
    try {
      const id = await invoke<string>("fork_chat_session", {
        sourceSessionId: chat.sessionId,
        keepChatBubbles,
        sourceMessageId: null,
        boundary: "through_turn",
        ephemeral: true,
        excludeTurns: true,
        newSessionId: null,
      });
      projectFiles.setPanelOpen(false);
      setSideSessionId(id);
      setSideHostSessionId(chat.sessionId);
      setChatRightOpen(false);
    } catch (error) {
      showTransientToast(String(error), { tone: "error" });
    }
  }, [
    activeProvider,
    chat.messages,
    chat.sessionId,
    chat.streaming,
    projectFiles.setPanelOpen,
    setChatRightOpen,
    showTransientToast,
    sideSessionId,
  ]);

  const projectPanelWasOpenRef = useRef(false);
  useEffect(() => {
    const justOpened = projectFiles.panelOpen && !projectPanelWasOpenRef.current;
    projectPanelWasOpenRef.current = projectFiles.panelOpen;
    if (!justOpened) return;
    setChatRightOpen(false);
    if (sideSessionId) void closeSideChat();
  }, [closeSideChat, projectFiles.panelOpen, setChatRightOpen, sideSessionId]);

  useEffect(() => {
    if (sideSessionId && sideHostSessionId && chat.sessionId !== sideHostSessionId) {
      void closeSideChat();
    }
  }, [chat.sessionId, closeSideChat, sideHostSessionId, sideSessionId]);
  const switchActiveProject = useCallback(
    (projectId: string) => {
      if (projectId === activeProjectId) return true;
      const hasDirtyFile = projectFiles.tabs.some(
        (tab) => !tab.readonly && tab.content !== tab.savedContent,
      );
      if (
        hasDirtyFile &&
        !window.confirm("当前项目还有未保存文件，切换项目会丢弃这些修改。仍要继续吗？")
      ) {
        return false;
      }
      setActiveProjectId(projectId);
      return true;
    },
    [activeProjectId, projectFiles.tabs],
  );

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
          setSkillsInitialTab("mcp");
          setNav("skills");
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
          openChatRightDock("context");
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
      openChatRightDock,
      openSettingsTab,
    ],
  );

  // ── Layout helpers ────────────────────────────────────────────────────────
  const ActiveIcon = IconChat;
  const showHeaderStatus = chat.statusPhase !== "ready";
  const featureNav =
    nav === "cron" || nav === "loop" || nav === "skills" ? nav : null;
  const FeatureIcon = featureNav
    ? (NAV.find((item) => item.id === featureNav)?.Icon ?? IconChat)
    : IconChat;
  const conversationTitle = useActiveSessionTitle(chat.sessionId);
  const { label: settingsTitle, Icon: SettingsIcon } = settingsTabMeta(settingsTab);
  const activeChatRightDock = resolveChatRightDock({
    projectFilesOpen: projectFiles.panelOpen,
    sideSessionOpen: Boolean(sideSessionId),
    inspectorOpen: chat.chatRightOpen,
  });
  const hasChatRightDock = activeChatRightDock !== null;
  const chatHeaderRightOffset = chatRightDockWidth(activeChatRightDock, {
    projectFiles: projectFilesWidth,
    inspector: chatRightPanelWidth,
  });

  useEffect(() => {
    if (!conversationMenuOpen) return;
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!conversationMenuRef.current?.contains(event.target as Node)) {
        setConversationMenuOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setConversationMenuOpen(false);
    };
    window.addEventListener("pointerdown", closeOnPointerDown);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointerDown);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [conversationMenuOpen]);

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

      <div
        className="body-row"
        style={{ "--sidebar-w-wide": `${sidebar.sidebarWidth}px` } as CSSProperties}
      >
        <aside
          ref={sidebar.sidebarRef}
          className={`sidebar ${sidebar.sidebarOpen || sidebar.sidebarPinned ? "is-open" : "is-collapsed"} ${sidebar.sidebarPinned ? "is-pinned" : ""} ${sidebar.showSidebarLabels ? "is-labels" : "is-icons"} ${sidebar.sidebarResizing ? "is-resizing" : ""}`}
          onMouseEnter={sidebar.openSidebar}
          onMouseLeave={sidebar.scheduleHideSidebar}
          onContextMenu={sidebar.openSidebarContextMenu}
        >
          {nav === "settings" ? (
            <>
              <button
                type="button"
                className="sidebar-back-btn"
                onClick={() => setNav("chat")}
              >
                <ArrowLeft size={16} strokeWidth={2} aria-hidden />
                <span className="sidebar-item-label">返回</span>
              </button>
              <div className="sidebar-settings-nav">
                {SETTINGS_TABS.map((item) => (
                  <button
                    key={item.id}
                    type="button"
                    className={`settings-sidebar-item ${settingsTab === item.id ? "is-active" : ""}`}
                    onClick={() => setSettingsTab(item.id)}
                  >
                    <item.Icon size={18} strokeWidth={1.6} aria-hidden />
                    <span className="sidebar-item-label">{item.label}</span>
                  </button>
                ))}
              </div>
            </>
          ) : (
            <>
              <div className="sidebar-brand">
                <div className="sidebar-logo" aria-hidden>
                  <AstroLogoMark width={26} height={26} />
                </div>
                <div className="sidebar-brand-text">Astro Agent</div>
              </div>
              <div className="sidebar-primary-actions">
                <button
                  type="button"
                  className="sidebar-new-chat"
                  onClick={() => {
                    setNav("chat");
                    void startNewChat();
                  }}
                  title={t("sidebar.newChat")}
                  aria-label={t("sidebar.newChat")}
                  aria-keyshortcuts="Meta+N Control+N"
                >
                  <IconNewChat width={18} height={18} strokeWidth={1.8} />
                  <span className="sidebar-item-label">{t("sidebar.newChat")}</span>
                </button>
                <ExpandableSearch
                  value={sessionQuery}
                  onChange={setSessionQuery}
                  placeholderKey="chat.rightPanel.searchSessions"
                  className="sidebar-session-search sidebar-global-search"
                />
              </div>
              <div className="sidebar-group-label">{t("sidebar.workspace")}</div>
              <nav className="sidebar-feature-tabs" aria-label={t("sidebar.features")}>
                {([
                  { id: "cron", label: t("nav.cron"), Icon: IconCron },
                  { id: "loop", label: t("nav.loop"), Icon: IconLoop },
                  { id: "skills", label: t("sidebar.plugins"), Icon: IconPlugin },
                ] as const).map(({ id, label, Icon }) => (
                  <button
                    key={id}
                    type="button"
                    className={`sidebar-feature-tab ${nav === id ? "is-active" : ""}`}
                    onClick={() => setNav(id)}
                    aria-current={nav === id ? "page" : undefined}
                    title={label}
                  >
                    <Icon width={18} height={18} strokeWidth={1.8} />
                    <span className="sidebar-item-label">{label}</span>
                  </button>
                ))}
              </nav>
              <div className="sidebar-projects">
                {searchingSessions ? (
                  <>
                    <div className="sidebar-section-header">
                      <span className="sidebar-section-title">
                        {t("sidebar.searchResults")}
                      </span>
                    </div>
                    <SidebarSessionList
                      activeSessionId={chat.sessionId}
                      sessionStatuses={sessionStatuses}
                      projectId={null}
                      query={sessionQuery}
                      listKind={sessionListKind}
                      onOpenSession={(sid) => void openSessionFromFilespace(sid)}
                      onPrepareDeleteCurrentSession={prepareDeleteCurrentSession}
                      onClearDeletedCurrentSession={clearDeletedCurrentSession}
                    />
                  </>
                ) : (
                  <>
                    {/* ── 置顶（无置顶会话时整个分区隐藏） ── */}
                    {pinnedCount > 0 && (
                      <div className="sidebar-collapsible-section">
                        <button
                          type="button"
                          className="sidebar-section-toggle"
                          onClick={() => toggleSection("pinned")}
                          aria-expanded={!collapsedSections.has("pinned")}
                        >
                          <span className="sidebar-section-title">{t("sessions.pin")}</span>
                          <ChevronRight
                            size={12}
                            strokeWidth={2}
                            className={`sidebar-section-chevron ${!collapsedSections.has("pinned") ? "is-expanded" : ""}`}
                            aria-hidden
                          />
                        </button>
                      </div>
                    )}
                    <div style={pinnedCount > 0 && !collapsedSections.has("pinned") ? undefined : { display: "none" }}>
                      <SidebarSessionList
                        activeSessionId={chat.sessionId}
                        sessionStatuses={sessionStatuses}
                        projectId={null}
                        query=""
                        listKind={sessionListKind}
                        pinnedFilter="pinned"
                        onCountChange={setPinnedCount}
                        onOpenSession={(sid) => void openSessionFromFilespace(sid)}
                        onPrepareDeleteCurrentSession={prepareDeleteCurrentSession}
                        onClearDeletedCurrentSession={clearDeletedCurrentSession}
                      />
                    </div>

                    {/* ── 项目 ── */}
                    <div className="sidebar-collapsible-section">
                      <button
                        type="button"
                        className="sidebar-section-toggle"
                        onClick={() => toggleSection("projects")}
                        aria-expanded={!collapsedSections.has("projects")}
                      >
                        <span className="sidebar-section-title">
                          {t("sidebar.projects")}
                        </span>
                        <ChevronRight
                          size={12}
                          strokeWidth={2}
                          className={`sidebar-section-chevron ${!collapsedSections.has("projects") ? "is-expanded" : ""}`}
                          aria-hidden
                        />
                      </button>
                      <div className="sidebar-section-actions">
                        <button
                          type="button"
                          className="sidebar-add-btn"
                          onClick={() => setProjectDialog({ mode: "create" })}
                          title="+ 新建项目"
                          aria-label="新建项目"
                        >
                          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                            <path d="M12 5v14" /><path d="M5 12h14" />
                          </svg>
                        </button>
                      </div>
                    </div>
                    {!collapsedSections.has("projects") && projects.map((proj) => (
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
                            title={proj.name}
                            onClick={() => {
                              if (activeProjectId !== proj.id) {
                                if (!switchActiveProject(proj.id)) return;
                                setCollapsedProjects((prev) => {
                                  const next = new Set(prev);
                                  next.delete(proj.id);
                                  return next;
                                });
                                setNav("chat");
                                void startNewChat();
                                return;
                              }
                              setCollapsedProjects((prev) => {
                                const next = new Set(prev);
                                if (next.has(proj.id)) next.delete(proj.id);
                                else next.add(proj.id);
                                return next;
                              });
                              if (!switchActiveProject(proj.id)) return;
                              setNav("chat");
                            }}
                          >
                            <ProjectFolderIcon
                              iconId={proj.icon}
                              expanded={!collapsedProjects.has(proj.id)}
                              size={18}
                            />
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
                            onClick={() => {
                              if (!switchActiveProject(proj.id)) return;
                              setNav("chat");
                              startNewChat();
                            }}
                            title={t("sidebar.newChat")}
                            aria-label={t("sidebar.newChat")}
                          >
                            <IconNewChat width={14} height={14} />
                          </button>
                        </div>
                        {!collapsedProjects.has(proj.id) && (
                          <SidebarSessionList
                            activeSessionId={chat.sessionId}
                            sessionStatuses={sessionStatuses}
                            projectId={proj.id}
                            query=""
                            listKind={sessionListKind}
                            onOpenSession={(sid) => {
                              if (!switchActiveProject(proj.id)) return;
                              void openSessionFromFilespace(sid);
                            }}
                            onPrepareDeleteCurrentSession={prepareDeleteCurrentSession}
                            onClearDeletedCurrentSession={clearDeletedCurrentSession}
                          />
                        )}
                      </div>
                    ))}

                    {/* ── 最近 ── */}
                    <div className="sidebar-collapsible-section">
                      <button
                        type="button"
                        className="sidebar-section-toggle"
                        onClick={() => toggleSection("recent")}
                        aria-expanded={!collapsedSections.has("recent")}
                      >
                        <span className="sidebar-section-title">
                          {sessionListKind === "archived" ? t("sessions.archived") : t("sidebar.recent")}
                        </span>
                        <ChevronRight
                          size={12}
                          strokeWidth={2}
                          className={`sidebar-section-chevron ${!collapsedSections.has("recent") ? "is-expanded" : ""}`}
                          aria-hidden
                        />
                      </button>
                      <div className="sidebar-section-actions">
                        <button
                          type="button"
                          className={`sidebar-session-filter-btn ${sessionListKind === "archived" ? "is-on" : ""}`}
                          title={sessionListKind === "archived" ? t("sessions.active") : t("sessions.archived")}
                          aria-label={sessionListKind === "archived" ? t("sessions.active") : t("sessions.archived")}
                          aria-pressed={sessionListKind === "archived"}
                          onClick={() =>
                            setSessionListKind((kind) => (kind === "archived" ? "active" : "archived"))
                          }
                        >
                          <Archive size={14} strokeWidth={1.8} aria-hidden />
                        </button>
                      </div>
                    </div>
                    {!collapsedSections.has("recent") && (
                      <SidebarSessionList
                        activeSessionId={chat.sessionId}
                        sessionStatuses={sessionStatuses}
                        projectId={null}
                        query=""
                        listKind={sessionListKind}
                        pinnedFilter="unpinned"
                        onOpenSession={(sid) => void openSessionFromFilespace(sid)}
                        onPrepareDeleteCurrentSession={prepareDeleteCurrentSession}
                        onClearDeletedCurrentSession={clearDeletedCurrentSession}
                      />
                    )}
                  </>
                )}
              </div>
              <div className="sidebar-footer">
                <button
                  type="button"
                  className="sidebar-settings-btn"
                  onClick={() => setNav("settings")}
                  title={t("nav.settings")}
                >
                  <Settings2 size={18} strokeWidth={1.8} aria-hidden />
                  <span className="sidebar-item-label">{t("nav.settings")}</span>
                  {chat.memoryPendingCount > 0 && (
                    <span className="nav-badge">
                      {chat.memoryPendingCount > 99 ? "99+" : String(chat.memoryPendingCount)}
                    </span>
                  )}
                </button>
              </div>
            </>
          )}
        </aside>
        {sidebar.showSidebarLabels ? (
          <button
            type="button"
            className={`sidebar-resizer${sidebar.sidebarResizing ? " is-resizing" : ""}`}
            role="separator"
            aria-label={t("sidebar.resize")}
            aria-orientation="vertical"
            aria-valuemin={sidebar.sidebarMinWidth}
            aria-valuemax={sidebar.sidebarMaxWidth}
            aria-valuenow={sidebar.sidebarWidth}
            title={t("sidebar.resize")}
            onDoubleClick={sidebar.resetSidebarWidth}
            onKeyDown={sidebar.onSidebarResizeKeyDown}
            onMouseEnter={sidebar.openSidebar}
            onMouseLeave={sidebar.scheduleHideSidebar}
            onPointerDown={sidebar.onSidebarResizePointerDown}
            onPointerMove={sidebar.onSidebarResizePointerMove}
            onPointerUp={sidebar.onSidebarResizePointerUp}
            onPointerCancel={(event) => sidebar.finishSidebarResize(event.pointerId)}
            onLostPointerCapture={(event) => sidebar.finishSidebarResize(event.pointerId)}
          />
        ) : null}
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
          {nav === "settings" ? (
            <>
              <div className="content-header">
                <div className="content-heading">
                  <div className="page-title-block">
                    <div className="page-title-icon" data-tone="twilight" aria-hidden>
                      <SettingsIcon width={15} height={15} strokeWidth={1.6} />
                    </div>
                    <div className="page-title-text">
                      <h1 className="content-title" data-tone="twilight">
                        <span className="content-title-main">{settingsTitle}</span>
                      </h1>
                    </div>
                  </div>
                </div>
              </div>
              <div className="page-body">
                <div className="settings-content-inline">
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
                    />
                  )}
                </div>
              </div>
            </>
          ) : featureNav ? (
            <>
              {featureNav !== "cron" && (
                <div className="content-header">
                  <div className="content-heading">
                    <div className="page-title-block">
                      <div className="page-title-icon" data-tone={shellTone} aria-hidden>
                        <FeatureIcon width={15} height={15} />
                      </div>
                      <div className="page-title-text">
                        <h1 className="content-title" data-tone={shellTone}>
                          <span className="content-title-main">
                            {t(PAGE_META[featureNav].titleKey)}
                          </span>
                        </h1>
                      </div>
                    </div>
                  </div>
                </div>
              )}
              <div
                className={`page-body${featureNav === "cron" ? " page-body--bare" : ""}`}
              >
                <div className="feature-content-inline">
                  {featureNav === "cron" && (
                    <CronPanel
                      active
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
                  {featureNav === "loop" && (
                    <LoopPanel
                      active
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
                  {featureNav === "skills" && (
                    <PluginsPage
                      active
                      initialTab={skillsInitialTab}
                      onInitialTabConsumed={() => setSkillsInitialTab(null)}
                      onInstallWithAgent={(prompt) => {
                        setInput(prompt);
                        setNav("chat");
                      }}
                      tone={shellTone}
                    />
                  )}
                </div>
              </div>
            </>
          ) : (
            <>
              <div
                className={`content-header content-header--chat${hasChatRightDock ? " has-right-dock" : ""}`}
                style={{
                  "--chat-header-right-offset": `${chatHeaderRightOffset}px`,
                } as CSSProperties}
              >
                <div className="content-heading">
                  {conversationTitle && (
                    <>
                      <div className="page-title-block">
                        <div className="page-title-icon" data-tone={shellTone} aria-hidden>
                          <ActiveIcon width={15} height={15} />
                        </div>
                        <div className="page-title-text">
                          <h1 className="content-title conversation-title" data-tone={shellTone}>
                            <span className="content-title-main" title={conversationTitle}>
                              {conversationTitle}
                            </span>
                          </h1>
                        </div>
                      </div>
                      <div className="conversation-menu" ref={conversationMenuRef}>
                        <button
                          type="button"
                          className={`conversation-menu-trigger ${conversationMenuOpen ? "is-open" : ""}`}
                          aria-label={t("sessions.moreActions")}
                          aria-haspopup="menu"
                          aria-expanded={conversationMenuOpen}
                          onClick={() => setConversationMenuOpen((open) => !open)}
                        >
                          <MoreHorizontal size={16} strokeWidth={2} />
                        </button>
                        {conversationMenuOpen && (
                          <div className="conversation-menu-popover" role="menu">
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => {
                                setConversationMenuOpen(false);
                                void startNewChat();
                              }}
                            >
                              <IconNewChat width={15} height={15} />
                              <span>{t("chat.newSession")}</span>
                            </button>
                            <button
                              type="button"
                              role="menuitem"
                              disabled={chat.streaming || chat.isCompacting}
                              onClick={() => {
                                setConversationMenuOpen(false);
                                void runCompactSession();
                              }}
                            >
                              <Sparkles size={15} strokeWidth={1.9} />
                              <span>{t("chat.slashCompact")}</span>
                            </button>
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => {
                                setConversationMenuOpen(false);
                                openChatRightDock("context");
                              }}
                            >
                              <IconRightPanel width={15} height={15} />
                              <span>{t("chat.rightPanel.context")}</span>
                            </button>
                          </div>
                        )}
                      </div>
                    </>
                  )}
                </div>
                <div className="header-actions">
                  {showHeaderStatus && (
                    <span className="status-chip status-chip--transient">
                      <span className={`status-dot ${chat.status}`} />
                      {statusText}
                    </span>
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
                      className={`header-icon-btn ${activeChatRightDock === "project-files" ? "is-active" : ""}`}
                      onClick={toggleProjectFilesDock}
                      title="展开项目文件"
                      aria-label="展开项目文件"
                      aria-pressed={activeChatRightDock === "project-files"}
                    >
                      <FolderTree width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${activeChatRightDock === "side-chat" ? "is-active" : ""}`}
                      onClick={() =>
                        void (sideSessionId ? closeSideChat() : startSideChat())
                      }
                      title={sideSessionId ? t("chat.side.close") : t("chat.side.open")}
                      aria-label={sideSessionId ? t("chat.side.close") : t("chat.side.open")}
                      aria-pressed={activeChatRightDock === "side-chat"}
                      disabled={!sideSessionId && (!chat.sessionId || chat.streaming)}
                    >
                      <MessageSquare width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${activeChatRightDock === "inspector" ? "is-active" : ""}`}
                      onClick={toggleChatRightDock}
                      title={t("chat.rightPanel.toggle")}
                      aria-label={t("chat.rightPanel.toggle")}
                      aria-pressed={activeChatRightDock === "inspector"}
                    >
                      <IconRightPanel width={16} height={16} />
                    </button>
                  </div>
                </div>
              </div>
              <div className="page-body page-body--chat">
                <div
                  className={`chat-layout-with-right${activeChatRightDock === "project-files" ? " has-project-files" : ""}${activeChatRightDock === "side-chat" ? " has-side-chat" : ""}${activeChatRightDock === "inspector" ? " has-chat-right" : ""}${hasChatRightDock ? " has-right-dock" : ""}`}
                  style={{
                    "--project-files-current-width": `${projectFilesWidth}px`,
                  } as CSSProperties}
                >
                  <div className="chat-main">
                    {chat.sessionEphemeral && (
                      <div className="chat-side-banner" role="status">
                        <span className="chat-side-mark" aria-hidden>
                          <MessageSquare size={14} />
                        </span>
                        <div>
                          <strong>{t("chat.side.banner")}</strong>
                          <span>
                            {t("chat.side.hiddenTurns", {
                              count: String(chat.sideExcludedTurnCount),
                            })}
                          </span>
                        </div>
                        {chat.sideParentSessionId && (
                          <button
                            type="button"
                            onClick={() =>
                              void openSessionFromFilespace(chat.sideParentSessionId!)
                            }
                          >
                            <ArrowLeft size={13} aria-hidden />
                            {t("chat.side.exit")}
                          </button>
                        )}
                      </div>
                    )}
                    <ChatView
                      sessionId={chat.sessionId}
                      messages={chat.messages}
                      workspaceContent={
                        projectFiles.tabs.length > 0 ? (
                          <ProjectFileEditor workbench={projectFiles} theme={resolved} />
                        ) : null
                      }
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
                      onPickWelcomePrompt={(prompt) => setInput(prompt)}
                      showThinkingControls={showThinking}
                      reasoningMeta={activeModelReasoning}
                      thinkingPrefs={thinkingPrefs}
                      onToggleThinking={onToggleThinking}
                      onThinkingLevelChange={onThinkingLevelChange}
                      onOpenMcpSettings={() => {
                        setSkillsInitialTab("mcp");
                        setNav("skills");
                      }}
                      chatMode={chatMode}
                      onChatModeChange={onChatModeChange}
                      onOpenContext={() => {
                        openChatRightDock("context");
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
                  <ProjectFilesPanel
                    open={activeChatRightDock === "project-files"}
                    workbench={projectFiles}
                    onWidthChange={setProjectFilesWidth}
                  />
                  {activeChatRightDock === "side-chat" && sideSessionId && activeProvider && (
                    <SideChatPanel
                      sessionId={sideSessionId}
                      provider={activeProvider}
                      interactionMode={chatMode}
                      onClose={closeSideChat}
                    />
                  )}
                  {activeChatRightDock === "inspector" && (
                    <ChatRightPanel
                      tab={chat.chatRightTab}
                      onTabChange={setChatRightTab}
                      onClose={() => setChatRightOpen(false)}
                      sessionId={chat.sessionId}
                      turnId={chat.currentTurnId}
                      tokenUsage={chat.tokenUsage}
                      contextUsage={chat.contextUsage}
                      contextWindow={contextWindow}
                      messages={chat.messages}
                      streaming={chat.streaming}
                      onOpenSession={(sessionId) => openSessionFromFilespace(sessionId)}
                      onOpenSideSession={(sessionId) => {
                        projectFiles.setPanelOpen(false);
                        setSideSessionId(sessionId);
                        setSideHostSessionId(chat.sessionId);
                        setChatRightOpen(false);
                      }}
                      onPrefillInput={setInput}
                      onOpenMemory={() => openSettingsTab("memory")}
                      onOpenSkills={() => setNav("skills")}
                      onWidthChange={setChatRightPanelWidth}
                    />
                  )}
                </div>
              </div>
            </>
          )}
        </section>
      </div>

      {projectMenu && (
        <ProjectContextMenu
          x={projectMenu.x}
          y={projectMenu.y}
          projectName={projectMenu.name}
          projectPath={projects.find((p) => p.id === projectMenu.id)?.roots[0] ?? ""}
          canRemove={projectMenu.id !== "default"}
          onAction={(action) => {
            if (action === "remove") {
              if (projectMenu.id === "default") return;
              void invoke("delete_project", { projectId: projectMenu.id }).catch(() => {});
              setProjects((prev) => prev.filter((p) => p.id !== projectMenu.id));
              if (activeProjectId === projectMenu.id) {
                setActiveProjectId(projects[0]?.id ?? "default");
              }
            } else if (action === "reveal") {
              const root = projects.find((p) => p.id === projectMenu.id)?.roots[0];
              if (root) {
                void import("@tauri-apps/plugin-opener").then((mod) =>
                  mod.revealItemInDir(root)
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
              if (proj) setProjectDialog({ mode: "edit", project: proj });
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
        open={projectDialog !== null}
        project={projectDialog?.mode === "edit" ? projectDialog.project : null}
        onClose={() => setProjectDialog(null)}
        onCreated={(created) => {
          setProjects((prev) => [...prev, created]);
          switchActiveProject(created.id);
        }}
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
