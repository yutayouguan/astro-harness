/** 根布局：侧栏导航、聊天与各功能面板编排。 */
import {
  lazy,
  Suspense,
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
import { AnimatePresence } from "framer-motion";

import AboutDialog from "./components/ui/AboutDialog";
import DynamicPaletteButton from "./components/ui/DynamicPaletteButton";
import ChatRightPanel, {
  type ChatRightTab,
} from "./components/chat/ChatRightPanel";
import ChatReviewPanel from "./components/chat/ChatReviewPanel";
import SideChatPanel from "./components/chat/SideChatPanel";
import ConversationTitle from "./components/chat/ConversationTitle";
import ProjectContextMenu from "./components/chat/ProjectContextMenu";
import ProjectEditDialog from "./components/chat/ProjectEditDialog";
import ProjectFolderIcon from "./components/chat/ProjectFolderIcon";
import SidebarSessionList from "./components/chat/SidebarSessionList";
import SessionActionsMenu from "./components/chat/SessionActionsMenu";
import { resolveSessionStatus } from "./components/chat/SessionStatusIcon";
import ChatView from "./components/chat/ChatView";
import BrowserDock from "./components/chat/BrowserDock";
import ProjectFileEditor, {
  ProjectFileTabs,
} from "./components/chat/ProjectFileEditor";
import ProjectFilesPanel from "./components/chat/ProjectFilesPanel";
import LoopPanel from "./components/loop/LoopPanel";
import CronPanel from "./components/schedule/CronPanel";
import InsightsPanel from "./components/settings/InsightsPanel";
import ModelMarketPanel from "./components/settings/ModelMarketPanel";
import MemoryPanel from "./components/settings/MemoryPanel";
import ModelPicker from "./components/agents/ModelPicker";
import ExpandableSearch from "./components/ui/ExpandableSearch";
import PreferencesPanel from "./components/settings/PreferencesPanel";
import BrowserSettingsPanel from "./components/settings/BrowserSettingsPanel";
import TerminalSettingsPanel from "./components/settings/TerminalSettingsPanel";
import EnvironmentDependenciesPanel from "./components/settings/EnvironmentDependenciesPanel";
import ProvidersPanel from "./components/settings/ProvidersPanel";
import SidebarContextMenu from "./components/settings/SidebarContextMenu";
import PluginsPage from "./components/plugins/PluginsPage";
import ToolsPanel from "./components/settings/ToolsPanel";
import EvolutionModelsPanel from "./components/settings/EvolutionModelsPanel";
import {
  AstroLogoMark,
  IconCron,
  IconLoop,
  IconNewChat,
  IconPanelClose,
  IconPanelOpen,
  IconPlugin,
  IconSearch,
  IconSettings,
} from "./components/icons";
import { useChatDisplayPrefs } from "./hooks/chat/useChatDisplayPrefs";
import { useActiveSessionMetadata } from "./hooks/chat/useActiveSessionTitle";
import { useChatSession } from "./hooks/chat/useChatSession";
import {
  useProjectFileWorkbench,
  type ProjectFileTab,
} from "./hooks/chat/useProjectFileWorkbench";
import { useSessionStatusMap } from "./hooks/chat/useSessionStatusMap";
import { useSubagentThreads } from "./hooks/chat/useSubagentThreads";
import { useChatThinkingPrefs } from "./hooks/chat/useChatThinkingPrefs";
import { cleanupStaleBrowserLiveWebviews } from "./hooks/chat/useBrowserLiveWebviews";
import { useBeautifyTips } from "./hooks/ui/useBeautifyTips";
import { usePrompt } from "./hooks/ui/DialogContext";
import { useProviders } from "./hooks/providers/useProviders";
import { useShellColorStyle } from "./hooks/app/useShellColorStyle";
import { useSidebar } from "./hooks/app/useSidebar";
import { useTheme } from "./hooks/app/useTheme";
import { useWallpaper } from "./hooks/app/useWallpaper";
import { useActiveUiStyle } from "./hooks/app/useActiveUiStyle";
import { useTransientToast } from "./hooks/ui/useTransientToast";
import { useDeferredPresence } from "./hooks/ui/useDeferredPresence";
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
import type { ComposerContextToken } from "./lib/chat/composerContext";
import type { FileChangeItem } from "./lib/chat/taskProgress";
import type { SessionListKind } from "./lib/chat/sessionManagement";
import { dispatchSessionsChanged } from "./lib/chat/sessionManagement";
import { resolveContextWindow, usagePercent } from "./lib/chat/contextUsage";
import {
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
  type MdMode,
} from "./lib/filespace/workspaceMdMode";
import { NAV, type NavId, type SettingsTabId } from "./lib/ui/navConfig";
import { SETTINGS_TAB_GROUPS, settingsTabMeta } from "./lib/ui/settingsTabs";
import { resolveChatRightDock } from "./lib/ui/chatRightDock";
import {
  Activity,
  ArrowLeft,
  Archive,
  ChevronRight,
  FolderTree,
  Globe2,
  History,
  MessageSquare,
  MoreHorizontal,
  Pin,
  SquareTerminal,
} from "lucide-react";
import { syncWindowUnderlay } from "./lib/ui/windowUnderlay";
import { dynamicGradientForTab } from "./lib/ui/dynamicGradient";
import { resolveMediaSrc } from "./lib/media/resolveMediaSrc";
import {
  applyWallpaperPaletteVars,
  clearWallpaperPaletteVars,
  resolveWallpaperPalette,
} from "./lib/ui/wallpaper";
import { resolveWallpaperPresentation } from "./lib/ui/activeUiStyle";
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

const TerminalDock = lazy(() => import("./components/chat/TerminalTabsDock"));
const ACTIVE_PROJECT_KEY = "astro.activeProjectId";

export default function App() {
  // ── Theme / i18n / prefs ──────────────────────────────────────────────────
  const { mode, setMode, resolved, setWallpaperTheme, reassert } = useTheme();
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
  const wallpaper = useWallpaper();
  const activeUiStyle = useActiveUiStyle();
  useBeautifyTips();
  const { t } = useI18n();
  const promptForTitle = usePrompt();
  const {
    prefs: chatDisplayPrefs,
    setVerbosity,
    setToggle,
    setAnswerLayout,
  } = useChatDisplayPrefs();
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
  const [settingsQuery, setSettingsQuery] = useState("");
  const visibleSettingsGroups = useMemo(() => {
    const query = settingsQuery.trim().toLocaleLowerCase();
    return SETTINGS_TAB_GROUPS.map((group) => {
      const label = t(group.labelKey);
      const items = group.items.map((item) => ({
        ...item,
        label: t(item.labelKey),
      }));
      return {
        ...group,
        label,
        items:
          !query || label.toLocaleLowerCase().includes(query)
            ? items
            : items.filter((item) =>
                item.label.toLocaleLowerCase().includes(query),
              ),
      };
    }).filter((group) => group.items.length > 0);
  }, [settingsQuery, t]);
  const [projects, setProjects] = useState<ProjectDto[]>([]);
  // 选中的项目决定工具执行目录，重启后必须沿用上次的选择，否则会退回默认空间。
  const [activeProjectId, setActiveProjectId] = useState(
    () => localStorage.getItem(ACTIVE_PROJECT_KEY) ?? "default",
  );
  const [collapsedProjects, setCollapsedProjects] = useState<Set<string>>(
    new Set(),
  );
  const [collapsedSections, setCollapsedSections] = useState<Set<string>>(
    new Set(),
  );
  const [pinnedCount, setPinnedCount] = useState(0);
  const [automationSessionCount, setAutomationSessionCount] = useState(0);
  const toggleSection = useCallback((section: string) => {
    setCollapsedSections((prev) => {
      const next = new Set(prev);
      if (next.has(section)) next.delete(section);
      else next.add(section);
      return next;
    });
  }, []);
  const [projectMenu, setProjectMenu] = useState<{
    id: string;
    name: string;
    x: number;
    y: number;
  } | null>(null);
  const [projectDialog, setProjectDialog] = useState<
    { mode: "create" } | { mode: "edit"; project: ProjectDto } | null
  >(null);
  const [conversationMenuAnchor, setConversationMenuAnchor] = useState<{
    x: number;
    y: number;
  } | null>(null);
  // 侧栏会话检索：搜索与归档视图跨全部项目生效
  const [sessionQuery, setSessionQuery] = useState("");
  const [sessionListKind, setSessionListKind] =
    useState<SessionListKind>("active");
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
  const [toolsInitialTab, setToolsInitialTab] = useState<"builtin" | null>(
    null,
  );
  const [skillsInitialTab, setSkillsInitialTab] = useState<"mcp" | null>(null);
  const [composerContextPrefill, setComposerContextPrefill] =
    useState<ComposerContextToken | null>(null);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [modelContextWindow, setModelContextWindow] = useState<number | null>(
    null,
  );
  const [activeModelCapabilities, setActiveModelCapabilities] =
    useState<ModelCapabilities | null>(null);
  const [activeModelReasoning, setActiveModelReasoning] =
    useState<ModelReasoningMeta | null>(null);
  const [activeModelPricing, setActiveModelPricing] =
    useState<ModelPricingMeta | null>(null);
  const [chatHeaderHasUnderlay, setChatHeaderHasUnderlay] = useState(false);
  const [sidebarRailPreview, setSidebarRailPreview] = useState(false);
  // ── Extracted hooks ───────────────────────────────────────────────────────
  const sidebar = useSidebar();
  const sidebarContentExpanded =
    !sidebar.sidebarCompact &&
    (sidebar.showSidebarLabels || sidebarRailPreview);
  useEffect(() => {
    if (sidebar.sidebarCompact) setSidebarRailPreview(false);
  }, [sidebar.sidebarCompact]);
  const toggleVisibleSidebarSection = useCallback(
    (section: string) => {
      if (!sidebarContentExpanded) {
        setCollapsedSections((prev) => {
          if (!prev.has(section)) return prev;
          const next = new Set(prev);
          next.delete(section);
          return next;
        });
        if (!sidebar.sidebarCompact) setSidebarRailPreview(true);
        return;
      }
      toggleSection(section);
    },
    [sidebar.sidebarCompact, sidebarContentExpanded, toggleSection],
  );
  const toggleVisibleProject = useCallback(
    (projectId: string) => {
      setCollapsedProjects((prev) => {
        const next = new Set(prev);
        if (!sidebarContentExpanded) next.delete(projectId);
        else if (next.has(projectId)) next.delete(projectId);
        else next.add(projectId);
        return next;
      });
      if (!sidebarContentExpanded && !sidebar.sidebarCompact) {
        setSidebarRailPreview(true);
      }
    },
    [sidebar.sidebarCompact, sidebarContentExpanded],
  );
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
  const subagents = useSubagentThreads(chat.sessionId);
  const runningSubagentCount = useMemo(
    () =>
      subagents.threads.filter(
        (thread) =>
          thread.status.kind === "pending_init" ||
          thread.status.kind === "running",
      ).length,
    [subagents.threads],
  );
  const hasSubagentAttention = useMemo(
    () =>
      Boolean(subagents.error) ||
      Object.values(subagents.state.byPath).some(
        (node) => node.unread || node.thread.status.kind === "errored",
      ),
    [subagents.error, subagents.state.byPath],
  );
  const summaryButtonLabel =
    runningSubagentCount > 0
      ? t("chat.rightPanel.summaryRunning", {
          count: String(runningSubagentCount),
        })
      : hasSubagentAttention
        ? t("chat.rightPanel.summaryAttention")
        : t("chat.rightPanel.summary");
  const sessionStatuses = useSessionStatusMap();
  const {
    send,
    startNewChat,
    runCompactSession,
    undoLastExchange,
    stopStream,
    pauseStream,
    resumeStream,
    editUserMessage,
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
  const projectFiles = useProjectFileWorkbench(
    activeProject,
    chat.generatingPreview,
  );
  const [projectMdMode, setProjectMdMode] =
    useState<MdMode>(readWorkspaceMdMode);
  const changeProjectMdMode = useCallback((mode: MdMode) => {
    setProjectMdMode(mode);
    writeWorkspaceMdMode(mode);
  }, []);
  const [projectFilesWidth, setProjectFilesWidth] = useState(264);
  const [browserDockOpen, setBrowserDockOpen] = useState(false);
  const [browserExpanded, setBrowserExpanded] = useState(false);
  const [browserComposerHeight, setBrowserComposerHeight] = useState(50);
  const [browserComposerOverlayOpen, setBrowserComposerOverlayOpen] =
    useState(false);
  const [terminalDockOpen, setTerminalDockOpen] = useState(false);
  const [sideSessionId, setSideSessionId] = useState<string | null>(null);
  const [sideHostSessionId, setSideHostSessionId] = useState<string | null>(
    null,
  );
  const [reviewState, setReviewState] = useState<{
    files: FileChangeItem[];
    selectedPath: string;
  } | null>(null);
  const activeProjectRoot = activeProject?.roots.find(Boolean) ?? null;

  useEffect(() => {
    void cleanupStaleBrowserLiveWebviews();
  }, []);

  useEffect(() => {
    if (nav !== "chat") setBrowserDockOpen(false);
  }, [nav]);

  const closeSideChat = useCallback(async () => {
    const sideId = sideSessionId;
    setSideSessionId(null);
    setSideHostSessionId(null);
    if (!sideId) return;
    await invoke("discard_side_session", { sessionId: sideId }).catch(
      (error) => {
        console.warn("discard_side_session failed", error);
      },
    );
  }, [sideSessionId]);

  const openChatRightDock = useCallback(
    (tab?: ChatRightTab) => {
      setBrowserDockOpen(false);
      setReviewState(null);
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

  const toggleChatSummaryDock = useCallback(() => {
    if (chat.chatRightOpen && chat.chatRightTab === "summary") {
      setChatRightOpen(false);
      return;
    }
    openChatRightDock("summary");
  }, [
    chat.chatRightOpen,
    chat.chatRightTab,
    openChatRightDock,
    setChatRightOpen,
  ]);

  const toggleProjectFilesDock = useCallback(() => {
    if (projectFiles.panelOpen) {
      projectFiles.setPanelOpen(false);
      return;
    }
    setChatRightOpen(false);
    setBrowserDockOpen(false);
    setReviewState(null);
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
    if (!chat.sessionId || !activeProvider || chat.streaming || sideSessionId)
      return;
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
      setBrowserDockOpen(false);
      setReviewState(null);
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

  const openFileReview = useCallback(
    (file: FileChangeItem, files: FileChangeItem[]) => {
      projectFiles.setPanelOpen(false);
      setBrowserDockOpen(false);
      setChatRightOpen(false);
      if (sideSessionId) void closeSideChat();
      setReviewState({ files, selectedPath: file.path });
    },
    [closeSideChat, projectFiles.setPanelOpen, setChatRightOpen, sideSessionId],
  );

  useEffect(() => {
    setReviewState(null);
  }, [activeProjectId, chat.sessionId]);

  const projectPanelWasOpenRef = useRef(false);
  useEffect(() => {
    const justOpened =
      projectFiles.panelOpen && !projectPanelWasOpenRef.current;
    projectPanelWasOpenRef.current = projectFiles.panelOpen;
    if (!justOpened) return;
    setBrowserDockOpen(false);
    setChatRightOpen(false);
    if (sideSessionId) void closeSideChat();
  }, [closeSideChat, projectFiles.panelOpen, setChatRightOpen, sideSessionId]);

  const toggleBrowserDock = useCallback(() => {
    if (browserDockOpen) {
      setBrowserDockOpen(false);
      return;
    }
    projectFiles.setPanelOpen(false);
    setChatRightOpen(false);
    setReviewState(null);
    if (sideSessionId) void closeSideChat();
    setBrowserDockOpen(true);
  }, [
    browserDockOpen,
    closeSideChat,
    projectFiles.setPanelOpen,
    setChatRightOpen,
    sideSessionId,
  ]);

  const previewProjectFileInBrowser = useCallback(
    async (tab: ProjectFileTab) => {
      if (!chat.sessionId || !activeProjectId || !tab.path) return;
      try {
        const result = await invoke<Record<string, unknown>>(
          "browser_preview_project_file",
          {
            request: {
              sessionId: chat.sessionId,
              projectId: activeProjectId,
              path: tab.path,
              content: tab.content,
            },
          },
        );
        chat.applyBrowserResult(result, "preview_file");
        projectFiles.setPanelOpen(false);
        setChatRightOpen(false);
        setReviewState(null);
        if (sideSessionId) void closeSideChat();
        setBrowserDockOpen(true);
      } catch (error) {
        showTransientToast(String(error), { tone: "error" });
      }
    },
    [
      activeProjectId,
      chat.applyBrowserResult,
      chat.sessionId,
      closeSideChat,
      projectFiles.setPanelOpen,
      setChatRightOpen,
      showTransientToast,
      sideSessionId,
    ],
  );

  useEffect(() => {
    if (!chat.browserPreview || chat.browserPreview.status === "closed") return;
    projectFiles.setPanelOpen(false);
    setChatRightOpen(false);
    setReviewState(null);
    if (sideSessionId) void closeSideChat();
    setBrowserDockOpen(true);
  }, [
    chat.browserPreview?.updatedAt,
    closeSideChat,
    projectFiles.setPanelOpen,
    setChatRightOpen,
    sideSessionId,
  ]);

  useEffect(() => {
    if (
      sideSessionId &&
      sideHostSessionId &&
      chat.sessionId !== sideHostSessionId
    ) {
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
        !window.confirm(
          "当前项目还有未保存文件，切换项目会丢弃这些修改。仍要继续吗？",
        )
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
    return () => {
      cancelled = true;
    };
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

  // ── ⌘/Ctrl+J：打开或折叠项目共享终端 ────────────────────────────
  useEffect(() => {
    if (nav !== "chat" || !activeProjectRoot) return;
    const onKey = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.altKey || event.shiftKey)
        return;
      if (event.key.toLowerCase() !== "j" || event.isComposing) return;
      const target = event.target as HTMLElement | null;
      if (target?.closest("input, textarea, [contenteditable='true']")) return;
      event.preventDefault();
      setTerminalDockOpen((open) => !open);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeProjectRoot, nav]);

  // ── macOS open-preferences / open-about listener ─────────────────────────
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window))
      return;
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
  const usesShellGradient =
    colorStyle === "unified" || colorStyle === "dynamic";
  const shellTone = usesShellGradient ? "blue" : activeTone;
  const activeShellGradient = useMemo(() => {
    if (colorStyle === "unified") return gradient;
    if (colorStyle === "dynamic") {
      return dynamicGradientForTab(dynamicSeed, nav, resolved);
    }
    return null;
  }, [colorStyle, gradient, dynamicSeed, nav, resolved]);
  const wallpaperPresentation = resolveWallpaperPresentation(
    activeUiStyle.style,
    wallpaper.prefs,
  );
  const generatedWallpaper = wallpaperPresentation.generated
    ? wallpaperPresentation.wallpaper
    : null;
  const effectiveWallpaper = wallpaperPresentation.wallpaper;
  const wallpaperSrc = effectiveWallpaper
    ? resolveMediaSrc(effectiveWallpaper.path)
    : null;
  const wallpaperEnabled = Boolean(wallpaperSrc);
  const wallpaperAdaptiveColor = wallpaperPresentation.adaptiveColor;
  const recommendedWallpaperTheme = wallpaperEnabled
    ? (effectiveWallpaper?.recommendedTheme ?? null)
    : null;
  const wallpaperPalette = wallpaperPresentation.generated
    ? wallpaperAdaptiveColor &&
      effectiveWallpaper?.accentColor &&
      effectiveWallpaper.secondaryColor
      ? {
          themeColor: effectiveWallpaper.accentColor,
          highlightColor: effectiveWallpaper.secondaryColor,
        }
      : null
    : resolveWallpaperPalette(wallpaper.prefs, effectiveWallpaper);
  const wallpaperThemeColor = wallpaperPalette?.themeColor ?? null;
  const wallpaperHighlightColor = wallpaperPalette?.highlightColor ?? null;
  useEffect(() => {
    setWallpaperTheme(recommendedWallpaperTheme);
  }, [recommendedWallpaperTheme, setWallpaperTheme]);
  useEffect(
    () => () => {
      setWallpaperTheme(null);
    },
    [setWallpaperTheme],
  );
  useEffect(() => {
    if (generatedWallpaper && !wallpaperSrc) {
      void activeUiStyle.reset();
      return;
    }
    if (
      wallpaper.prefs.mode === "wallpaper" &&
      wallpaper.prefs.current &&
      !wallpaperSrc
    ) {
      wallpaper.markCurrentUnavailable();
    }
  }, [
    wallpaper.prefs.mode,
    wallpaper.prefs.current,
    wallpaper.markCurrentUnavailable,
    wallpaperSrc,
    generatedWallpaper,
    activeUiStyle.reset,
  ]);
  // ── Tone crossfade overlay ─────────────────────────────────────────────
  const prevToneRef = useRef(shellTone);
  const prevDynamicSeedRef = useRef(dynamicSeed);
  const toneFadeRevisionRef = useRef(0);
  const [toneFade, setToneFade] = useState<{
    background: string;
    revision: number;
  } | null>(null);
  const shellRef = useRef<HTMLDivElement | null>(null);

  // layout：在绘制前写 CSS 变量，并刷新玻璃层（避免 backdrop-filter 缓存旧色）
  useLayoutEffect(() => {
    const root = document.documentElement;
    const colorfulToneChanged =
      prevToneRef.current !== shellTone &&
      colorStyle === "colorful" &&
      !wallpaperEnabled;
    const dynamicPaletteChanged =
      prevDynamicSeedRef.current !== dynamicSeed &&
      colorStyle === "dynamic" &&
      !wallpaperEnabled;
    if ((colorfulToneChanged || dynamicPaletteChanged) && shellRef.current) {
      const bg = getComputedStyle(shellRef.current).background;
      if (bg) {
        toneFadeRevisionRef.current += 1;
        setToneFade({ background: bg, revision: toneFadeRevisionRef.current });
      }
    }
    prevToneRef.current = shellTone;
    prevDynamicSeedRef.current = dynamicSeed;
    root.setAttribute("data-tone", shellTone);
    root.setAttribute("data-color-style", colorStyle);
    if (wallpaperEnabled) {
      root.setAttribute("data-wallpaper", "true");
    } else {
      root.removeAttribute("data-wallpaper");
    }
    if (wallpaperEnabled && wallpaperThemeColor && wallpaperHighlightColor) {
      root.setAttribute("data-wallpaper-palette", "true");
      applyWallpaperPaletteVars(
        root,
        wallpaperThemeColor,
        wallpaperHighlightColor,
      );
      flushGlassBackdrop(root);
    } else {
      root.removeAttribute("data-wallpaper-palette");
      clearWallpaperPaletteVars(root);
    }
    if (activeShellGradient) {
      applyShellGradientVars(root, activeShellGradient, resolved);
      flushGlassBackdrop(root);
    } else {
      clearShellGradientVars(root);
    }
    reassert();
  }, [
    shellTone,
    colorStyle,
    dynamicSeed,
    activeShellGradient,
    resolved,
    reassert,
    wallpaperEnabled,
    wallpaperThemeColor,
    wallpaperHighlightColor,
  ]);
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
  const statusText =
    chat.statusDetail ?? t(`status.${chat.statusPhase}` as MessageKey);
  // ── Thinking callbacks ────────────────────────────────────────────────────
  const onThinkingLevelChange = useCallback(
    (level: ThinkingLevel) => {
      setThinkingLevel(level);
      syncMaxModeWithThinkingLevel(level);
      if (!activeProvider) return;
      upsertModelPrefs(
        activeProvider.id,
        activeProvider.model,
        thinkingLevelToModelPatch(level),
      );
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
              const rows = await invoke<
                { id: string; action: string; target: string; source: string }[]
              >("list_pending_memory_writes");
              if (!rows?.length) {
                showTransientToast(t("memory.pending.emptyTitle"));
                setMemoryPendingCount(0);
                return;
              }
              setMemoryPendingCount(rows.length);
              const lines = rows
                .slice(0, 5)
                .map(
                  (r) =>
                    `${r.id.slice(0, 8)} ${r.action}/${r.target} (${r.source})`,
                );
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
                  : await invoke<string>("approve_pending_memory_write", {
                      id,
                    });
              showTransientToast(msg || t("memory.pending.approved"));
              if (chat.sessionId) {
                try {
                  const settings = await invoke<{
                    autoRefreshOnUpdate: boolean;
                  }>("get_memory_settings");
                  if (settings.autoRefreshOnUpdate !== false) {
                    await invoke("refresh_memory", {
                      agentId: null,
                      sessionId: chat.sessionId,
                    });
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
                const msg = await invoke<string>(
                  "reject_all_pending_memory_writes",
                );
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
              await invoke("refresh_memory", {
                agentId: null,
                sessionId: chat.sessionId ?? null,
              });
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
  const showHeaderStatus = chat.statusPhase !== "ready";
  const featureNav =
    nav === "cron" || nav === "loop" || nav === "skills" ? nav : null;
  const activeSession = useActiveSessionMetadata(chat.sessionId);
  const conversationTitle = activeSession?.summary ?? null;
  useEffect(() => setConversationMenuAnchor(null), [chat.sessionId]);
  const renameConversation = useCallback(async () => {
    if (!chat.sessionId || !conversationTitle) return;
    const next = await promptForTitle({
      title: t("sessions.rename"),
      message: t("sessions.renamePrompt"),
      defaultValue: conversationTitle,
      confirmLabel: t("sessions.renameSave"),
      cancelLabel: t("sessions.cancel"),
    });
    const title = next?.trim();
    if (!title || title === conversationTitle) return;
    try {
      await invoke("rename_session", { sessionId: chat.sessionId, title });
      dispatchSessionsChanged();
    } catch (error) {
      showTransientToast(
        t("sessions.actionFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
        { tone: "error" },
      );
    }
  }, [
    chat.sessionId,
    conversationTitle,
    promptForTitle,
    showTransientToast,
    t,
  ]);
  const { labelKey: settingsTitleKey, Icon: SettingsIcon } =
    settingsTabMeta(settingsTab);
  const settingsTitle = t(settingsTitleKey);
  const activeChatRightDock = resolveChatRightDock({
    projectFilesOpen: projectFiles.panelOpen,
    browserOpen: browserDockOpen,
    sideSessionOpen: Boolean(sideSessionId),
    inspectorOpen: chat.chatRightOpen,
    reviewOpen: reviewState != null,
  });
  const hasChatRightDock = activeChatRightDock !== null;
  const browserOwnsTitlebar =
    nav === "chat" && activeChatRightDock === "browser";
  const browserDockPresence = useDeferredPresence(
    activeChatRightDock === "browser",
  );
  const terminalDockPresence = useDeferredPresence(
    terminalDockOpen && Boolean(activeProjectRoot && activeProject),
    { persistAfterOpen: true },
  );

  useEffect(() => {
    if (activeChatRightDock === "browser") return;
    setBrowserExpanded(false);
    setBrowserComposerHeight(50);
    setBrowserComposerOverlayOpen(false);
  }, [activeChatRightDock]);

  // ── JSX ───────────────────────────────────────────────────────────────────
  return (
    <div
      ref={shellRef}
      className={`app-shell ${winChrome.windowMaximized ? "is-maximized" : ""}${browserOwnsTitlebar ? " has-browser-surface" : ""}${wallpaperEnabled ? " has-wallpaper" : ""}`}
      data-tone={shellTone}
      data-color-style={colorStyle}
      data-sidebar-state={sidebar.sidebarVisible ? "visible" : "collapsed"}
    >
      {wallpaperSrc ? (
        <div
          className="shell-wallpaper-layer"
          style={
            {
              "--wallpaper-blur": wallpaperPresentation.blur,
              "--wallpaper-shade": wallpaperPresentation.shade / 100,
            } as CSSProperties
          }
          aria-hidden
        >
          <img
            src={wallpaperSrc}
            alt=""
            style={{
              objectFit:
                wallpaperPresentation.fit === "stretch"
                  ? "fill"
                  : wallpaperPresentation.fit,
            }}
            onError={
              generatedWallpaper
                ? () => void activeUiStyle.reset()
                : wallpaper.markCurrentUnavailable
            }
          />
          <span />
        </div>
      ) : null}
      {toneFade && (
        <div
          key={toneFade.revision}
          className="shell-tone-crossfade"
          style={{ background: toneFade.background }}
          onAnimationEnd={() =>
            setToneFade((current) =>
              current?.revision === toneFade.revision ? null : current,
            )
          }
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
          aria-label={
            sidebar.sidebarPinned
              ? t("sidebar.unpinAria")
              : t("sidebar.pinAria")
          }
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
        style={
          { "--sidebar-w-wide": `${sidebar.sidebarWidth}px` } as CSSProperties
        }
      >
        <aside
          ref={sidebar.sidebarRef}
          className={`sidebar ${nav === "settings" ? "is-settings" : ""} ${sidebar.sidebarOpen || sidebar.sidebarPinned ? "is-open" : "is-collapsed"} ${sidebar.sidebarPinned ? "is-pinned" : ""} ${sidebarContentExpanded ? "is-labels" : "is-icons"} ${sidebar.sidebarCompact ? "is-compact" : ""} ${sidebarRailPreview ? "is-rail-preview" : ""} ${sidebar.sidebarResizing ? "is-resizing" : ""}`}
          onMouseEnter={sidebar.openSidebar}
          onMouseLeave={() => {
            sidebar.scheduleHideSidebar();
            setSidebarRailPreview(false);
          }}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) {
              setSidebarRailPreview(false);
            }
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape" && sidebarRailPreview) {
              event.stopPropagation();
              setSidebarRailPreview(false);
            }
          }}
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
                <span className="sidebar-item-label">
                  {t("settings.sidebar.back")}
                </span>
              </button>
              <label className="sidebar-settings-search">
                <IconSearch width={16} height={16} aria-hidden />
                <input
                  type="search"
                  value={settingsQuery}
                  onChange={(event) => setSettingsQuery(event.target.value)}
                  placeholder={t("settings.sidebar.searchPlaceholder")}
                  aria-label={t("settings.sidebar.searchAria")}
                />
              </label>
              <nav
                className="sidebar-settings-nav"
                aria-label={t("settings.sidebar.categoriesAria")}
              >
                {visibleSettingsGroups.map((group) => (
                  <div className="sidebar-settings-group" key={group.id}>
                    <div className="sidebar-settings-group-label">
                      {group.label}
                    </div>
                    {group.items.map((item) => (
                      <button
                        key={item.id}
                        type="button"
                        className={`settings-sidebar-item ${settingsTab === item.id ? "is-active" : ""}`}
                        aria-current={
                          settingsTab === item.id ? "page" : undefined
                        }
                        onClick={() => setSettingsTab(item.id)}
                      >
                        <span className="settings-sidebar-icon" aria-hidden>
                          <item.Icon width={18} height={18} strokeWidth={1.7} />
                        </span>
                        <span className="sidebar-item-label">{item.label}</span>
                      </button>
                    ))}
                  </div>
                ))}
                {visibleSettingsGroups.length === 0 ? (
                  <p className="sidebar-settings-empty">
                    {t("settings.sidebar.empty")}
                  </p>
                ) : null}
              </nav>
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
                  <span className="sidebar-item-label">
                    {t("sidebar.newChat")}
                  </span>
                  <kbd className="sidebar-new-chat-shortcut" aria-hidden>
                    ⌘N
                  </kbd>
                </button>
                <ExpandableSearch
                  value={sessionQuery}
                  onChange={setSessionQuery}
                  placeholderKey="chat.rightPanel.searchSessions"
                  className="sidebar-session-search sidebar-global-search"
                />
              </div>
              <div className="sidebar-group-label">
                {t("sidebar.workspace")}
              </div>
              <nav
                className="sidebar-feature-tabs"
                aria-label={t("sidebar.features")}
              >
                {(
                  [
                    { id: "cron", label: t("nav.cron"), Icon: IconCron },
                    { id: "loop", label: t("nav.loop"), Icon: IconLoop },
                    {
                      id: "skills",
                      label: t("sidebar.plugins"),
                      Icon: IconPlugin,
                    },
                  ] as const
                ).map(({ id, label, Icon }) => (
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
                      onOpenSession={(sid) =>
                        void openSessionFromFilespace(sid)
                      }
                      onPrepareDeleteCurrentSession={
                        prepareDeleteCurrentSession
                      }
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
                          onClick={() => toggleVisibleSidebarSection("pinned")}
                          aria-expanded={!collapsedSections.has("pinned")}
                          aria-label={t("sessions.pin")}
                          title={
                            sidebarContentExpanded
                              ? undefined
                              : t("sessions.pin")
                          }
                        >
                          <span className="sidebar-section-icon" aria-hidden>
                            <Pin size={17} strokeWidth={1.8} />
                          </span>
                          <span className="sidebar-section-title">
                            {t("sessions.pin")}
                          </span>
                          <ChevronRight
                            size={12}
                            strokeWidth={2}
                            className={`sidebar-section-chevron ${!collapsedSections.has("pinned") ? "is-expanded" : ""}`}
                            aria-hidden
                          />
                        </button>
                      </div>
                    )}
                    <div
                      style={
                        pinnedCount > 0 && !collapsedSections.has("pinned")
                          ? undefined
                          : { display: "none" }
                      }
                    >
                      <SidebarSessionList
                        activeSessionId={chat.sessionId}
                        sessionStatuses={sessionStatuses}
                        projectId={null}
                        query=""
                        listKind={sessionListKind}
                        placement="pinned"
                        onCountChange={setPinnedCount}
                        onOpenSession={(sid) =>
                          void openSessionFromFilespace(sid)
                        }
                        onPrepareDeleteCurrentSession={
                          prepareDeleteCurrentSession
                        }
                        onClearDeletedCurrentSession={
                          clearDeletedCurrentSession
                        }
                      />
                    </div>

                    {/* ── 项目 ── */}
                    <div className="sidebar-collapsible-section">
                      <button
                        type="button"
                        className="sidebar-section-toggle"
                        onClick={() => toggleVisibleSidebarSection("projects")}
                        aria-expanded={!collapsedSections.has("projects")}
                        aria-label={t("sidebar.projects")}
                        title={
                          sidebarContentExpanded
                            ? undefined
                            : t("sidebar.projects")
                        }
                      >
                        <span className="sidebar-section-icon" aria-hidden>
                          <FolderTree size={18} strokeWidth={1.8} />
                        </span>
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
                          <svg
                            width="14"
                            height="14"
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                            aria-hidden
                          >
                            <path d="M12 5v14" />
                            <path d="M5 12h14" />
                          </svg>
                        </button>
                      </div>
                    </div>
                    {!collapsedSections.has("projects") &&
                      projects.map((proj) => (
                        <div
                          key={proj.id}
                          className="sidebar-project"
                          onContextMenu={(e) => {
                            e.preventDefault();
                            e.stopPropagation();
                            setProjectMenu({
                              ...proj,
                              x: e.clientX,
                              y: e.clientY,
                            });
                          }}
                        >
                          <div className="sidebar-project-header">
                            <button
                              type="button"
                              className="sidebar-project-name"
                              title={proj.name}
                              aria-expanded={!collapsedProjects.has(proj.id)}
                              onClick={() => toggleVisibleProject(proj.id)}
                            >
                              <ProjectFolderIcon
                                iconId={proj.icon}
                                expanded={!collapsedProjects.has(proj.id)}
                                size={18}
                              />
                              <span className="sidebar-item-label">
                                {proj.name}
                              </span>
                            </button>
                            <button
                              type="button"
                              className="sidebar-project-more"
                              onClick={(e) => {
                                e.stopPropagation();
                                const rect =
                                  e.currentTarget.getBoundingClientRect();
                                setProjectMenu({
                                  ...proj,
                                  x: rect.right + 4,
                                  y: rect.top,
                                });
                              }}
                              title="更多"
                              aria-label="更多"
                            >
                              <svg
                                width="14"
                                height="14"
                                viewBox="0 0 24 24"
                                fill="currentColor"
                                aria-hidden
                              >
                                <circle cx="12" cy="5" r="1.5" />
                                <circle cx="12" cy="12" r="1.5" />
                                <circle cx="12" cy="19" r="1.5" />
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
                              placement="project"
                              onOpenSession={(sid) => {
                                if (!switchActiveProject(proj.id)) return;
                                void openSessionFromFilespace(sid);
                              }}
                              onPrepareDeleteCurrentSession={
                                prepareDeleteCurrentSession
                              }
                              onClearDeletedCurrentSession={
                                clearDeletedCurrentSession
                              }
                            />
                          )}
                        </div>
                      ))}

                    {/* ── 自动化运行（Cron 会话保留项目上下文，但在展示上独立分组） ── */}
                    {automationSessionCount > 0 && (
                      <div className="sidebar-collapsible-section">
                        <button
                          type="button"
                          className="sidebar-section-toggle"
                          onClick={() =>
                            toggleVisibleSidebarSection("automation")
                          }
                          aria-expanded={!collapsedSections.has("automation")}
                          aria-label={t("sidebar.automationRuns")}
                          title={
                            sidebarContentExpanded
                              ? undefined
                              : t("sidebar.automationRuns")
                          }
                        >
                          <span className="sidebar-section-icon" aria-hidden>
                            <Activity size={17} strokeWidth={1.8} />
                          </span>
                          <span className="sidebar-section-title">
                            {t("sidebar.automationRuns")}
                          </span>
                          <ChevronRight
                            size={12}
                            strokeWidth={2}
                            className={`sidebar-section-chevron ${!collapsedSections.has("automation") ? "is-expanded" : ""}`}
                            aria-hidden
                          />
                        </button>
                      </div>
                    )}
                    <div
                      style={
                        automationSessionCount > 0 &&
                        !collapsedSections.has("automation")
                          ? undefined
                          : { display: "none" }
                      }
                    >
                      <SidebarSessionList
                        activeSessionId={chat.sessionId}
                        sessionStatuses={sessionStatuses}
                        projectId={null}
                        query=""
                        listKind={sessionListKind}
                        placement="automation"
                        onCountChange={setAutomationSessionCount}
                        onOpenSession={(sid) =>
                          void openSessionFromFilespace(sid)
                        }
                        onPrepareDeleteCurrentSession={
                          prepareDeleteCurrentSession
                        }
                        onClearDeletedCurrentSession={
                          clearDeletedCurrentSession
                        }
                      />
                    </div>

                    {/* ── 最近 ── */}
                    <div className="sidebar-collapsible-section">
                      <button
                        type="button"
                        className="sidebar-section-toggle"
                        onClick={() => toggleVisibleSidebarSection("recent")}
                        aria-expanded={!collapsedSections.has("recent")}
                        aria-label={
                          sessionListKind === "archived"
                            ? t("sessions.archived")
                            : t("sidebar.recent")
                        }
                        title={
                          sidebarContentExpanded
                            ? undefined
                            : sessionListKind === "archived"
                              ? t("sessions.archived")
                              : t("sidebar.recent")
                        }
                      >
                        <span className="sidebar-section-icon" aria-hidden>
                          <History size={18} strokeWidth={1.8} />
                        </span>
                        <span className="sidebar-section-title">
                          {sessionListKind === "archived"
                            ? t("sessions.archived")
                            : t("sidebar.recent")}
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
                          title={
                            sessionListKind === "archived"
                              ? t("sessions.active")
                              : t("sessions.archived")
                          }
                          aria-label={
                            sessionListKind === "archived"
                              ? t("sessions.active")
                              : t("sessions.archived")
                          }
                          aria-pressed={sessionListKind === "archived"}
                          onClick={() =>
                            setSessionListKind((kind) =>
                              kind === "archived" ? "active" : "archived",
                            )
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
                        placement="recent"
                        onOpenSession={(sid) =>
                          void openSessionFromFilespace(sid)
                        }
                        onPrepareDeleteCurrentSession={
                          prepareDeleteCurrentSession
                        }
                        onClearDeletedCurrentSession={
                          clearDeletedCurrentSession
                        }
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
                  <IconSettings width={17} height={17} strokeWidth={1.8} />
                  <span className="sidebar-item-label">
                    {t("nav.settings")}
                  </span>
                  {chat.memoryPendingCount > 0 && (
                    <span className="nav-badge">
                      {chat.memoryPendingCount > 99
                        ? "99+"
                        : String(chat.memoryPendingCount)}
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
            onPointerCancel={(event) =>
              sidebar.finishSidebarResize(event.pointerId)
            }
            onLostPointerCapture={(event) =>
              sidebar.finishSidebarResize(event.pointerId)
            }
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

        <section
          className={`content-pane${nav === "chat" ? " content-pane--chat" : ""}`}
        >
          {nav === "settings" ? (
            <>
              <div className="content-header">
                <div className="content-heading">
                  <div className="page-title-block">
                    <div
                      className="page-title-icon"
                      data-tone="twilight"
                      aria-hidden
                    >
                      <SettingsIcon width={15} height={15} strokeWidth={1.6} />
                    </div>
                    <div className="page-title-text">
                      <h1 className="content-title" data-tone="twilight">
                        <span className="content-title-main">
                          {settingsTitle}
                        </span>
                      </h1>
                    </div>
                  </div>
                </div>
              </div>
              <div className="page-body">
                <div className="settings-content-inline">
                  {settingsTab.startsWith("preferences") && (
                    <PreferencesPanel
                      section={
                        settingsTab === "preferences"
                          ? "general"
                          : (settingsTab.split(
                              ":",
                            )[1] as import("./components/settings/PreferencesPanel").PreferenceCategory)
                      }
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
                      wallpaper={wallpaper}
                      tone={shellTone}
                      chatDisplayPrefs={chatDisplayPrefs}
                      onChatVerbosityChange={setVerbosity}
                      onChatAnswerLayoutChange={setAnswerLayout}
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
                  {settingsTab === "browser" && (
                    <BrowserSettingsPanel
                      active={nav === "settings"}
                      tone={shellTone}
                    />
                  )}
                  {settingsTab === "terminal" && (
                    <TerminalSettingsPanel tone={shellTone} />
                  )}
                  {settingsTab === "environment-dependencies" && (
                    <EnvironmentDependenciesPanel
                      active={nav === "settings"}
                      tone={shellTone}
                    />
                  )}
                  {settingsTab === "evolution" && (
                    <EvolutionModelsPanel
                      active={nav === "settings"}
                      tone={shellTone}
                    />
                  )}
                  {settingsTab === "insights" && (
                    <InsightsPanel active={nav === "settings"} />
                  )}
                  {settingsTab === "models" && (
                    <ModelMarketPanel
                      active={nav === "settings"}
                      onProvidersStateChange={syncProvidersFromState}
                    />
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
              <div className="page-body page-body--bare">
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
                      onInstallWithAgent={(prompt, contextToken) => {
                        setInput(prompt);
                        setComposerContextPrefill(contextToken ?? null);
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
                className={`content-header content-header--chat${hasChatRightDock ? " has-right-dock" : ""}${projectFiles.tabs.length > 0 ? " has-project-file" : ""}${browserExpanded ? " is-browser-expanded" : ""}${chat.emptyMode ? " is-welcome" : chatHeaderHasUnderlay || projectFiles.tabs.length > 0 ? " has-content-underlay" : ""}`}
              >
                <div className="content-heading">
                  {projectFiles.tabs.length > 0 ? (
                    <ProjectFileTabs
                      workbench={projectFiles}
                      mdMode={projectMdMode}
                      onMdModeChange={changeProjectMdMode}
                      onPreviewInBrowser={previewProjectFileInBrowser}
                    />
                  ) : conversationTitle ? (
                    <>
                      <div className="page-title-block">
                        <div
                          className="page-title-icon"
                          data-tone={shellTone}
                          aria-hidden
                        >
                          <ProjectFolderIcon
                            iconId={activeProject?.icon}
                            expanded={false}
                            size={18}
                            loading="eager"
                          />
                        </div>
                        <div className="page-title-text">
                          <h1
                            className="content-title conversation-title"
                            data-tone={shellTone}
                          >
                            <ConversationTitle
                              title={conversationTitle}
                              renameLabel={t("sessions.rename")}
                              onRename={() => void renameConversation()}
                            />
                          </h1>
                        </div>
                      </div>
                      <div className="conversation-menu">
                        <button
                          type="button"
                          className={`conversation-menu-trigger ${conversationMenuAnchor ? "is-open" : ""}`}
                          aria-label={t("sessions.moreActions")}
                          aria-haspopup="menu"
                          aria-expanded={Boolean(conversationMenuAnchor)}
                          onClick={(event) => {
                            if (conversationMenuAnchor) {
                              setConversationMenuAnchor(null);
                              return;
                            }
                            const rect =
                              event.currentTarget.getBoundingClientRect();
                            setConversationMenuAnchor({
                              x: rect.left,
                              y: rect.bottom + 6,
                            });
                          }}
                        >
                          <MoreHorizontal size={16} strokeWidth={2} />
                        </button>
                        {conversationMenuAnchor && activeSession && (
                          <SessionActionsMenu
                            session={activeSession}
                            x={conversationMenuAnchor.x}
                            y={conversationMenuAnchor.y}
                            status={resolveSessionStatus(
                              sessionStatuses[activeSession.sessionId],
                            )}
                            activeSessionId={chat.sessionId}
                            onClose={() => setConversationMenuAnchor(null)}
                            onOpenSession={(sessionId) => {
                              void openSessionFromFilespace(sessionId);
                            }}
                            showToast={showTransientToast}
                            onPrepareDeleteCurrentSession={
                              prepareDeleteCurrentSession
                            }
                            onClearDeletedCurrentSession={
                              clearDeletedCurrentSession
                            }
                          />
                        )}
                      </div>
                    </>
                  ) : null}
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
                      onClick={() => {
                        setNav("chat");
                        startNewChat();
                      }}
                      data-tip={t("sidebar.newChat")}
                      aria-label={t("sidebar.newChat")}
                    >
                      <svg
                        width="16"
                        height="16"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z" />
                        <path d="M12 8v8" />
                        <path d="M8 12h8" />
                      </svg>
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${activeChatRightDock === "project-files" ? "is-active" : ""}`}
                      onClick={toggleProjectFilesDock}
                      title={t("chat.projectFiles.open")}
                      aria-label={t("chat.projectFiles.open")}
                      aria-pressed={activeChatRightDock === "project-files"}
                    >
                      <FolderTree width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${activeChatRightDock === "browser" ? "is-active" : ""}`}
                      onClick={toggleBrowserDock}
                      title="打开内置浏览器"
                      aria-label="打开内置浏览器"
                      aria-pressed={activeChatRightDock === "browser"}
                    >
                      <Globe2 width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${terminalDockOpen ? "is-active" : ""}`}
                      onClick={() => setTerminalDockOpen((open) => !open)}
                      title={t("chat.terminal.toggle")}
                      aria-label={t("chat.terminal.toggle")}
                      aria-pressed={terminalDockOpen}
                      disabled={!activeProjectRoot}
                    >
                      <SquareTerminal width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn ${activeChatRightDock === "side-chat" ? "is-active" : ""}`}
                      onClick={() =>
                        void (sideSessionId ? closeSideChat() : startSideChat())
                      }
                      title={
                        sideSessionId
                          ? t("chat.side.close")
                          : t("chat.side.open")
                      }
                      aria-label={
                        sideSessionId
                          ? t("chat.side.close")
                          : t("chat.side.open")
                      }
                      aria-pressed={activeChatRightDock === "side-chat"}
                      disabled={
                        !sideSessionId && (!chat.sessionId || chat.streaming)
                      }
                    >
                      <MessageSquare width={16} height={16} />
                    </button>
                    <button
                      type="button"
                      className={`header-icon-btn header-summary-btn ${activeChatRightDock === "inspector" && chat.chatRightTab === "summary" ? "is-active" : ""} ${hasSubagentAttention ? "has-attention" : ""}`.trim()}
                      onClick={toggleChatSummaryDock}
                      title={summaryButtonLabel}
                      aria-label={summaryButtonLabel}
                      aria-pressed={
                        activeChatRightDock === "inspector" &&
                        chat.chatRightTab === "summary"
                      }
                    >
                      <Activity width={16} height={16} />
                      {runningSubagentCount > 0 ? (
                        <span className="header-summary-badge" aria-hidden>
                          {runningSubagentCount > 9
                            ? "9+"
                            : runningSubagentCount}
                        </span>
                      ) : hasSubagentAttention ? (
                        <span className="header-summary-dot" aria-hidden />
                      ) : null}
                    </button>
                  </div>
                </div>
              </div>
              <div
                className={`page-body page-body--chat${projectFiles.tabs.length > 0 ? " has-project-file" : ""}`}
              >
                <div
                  className={`chat-layout-with-right${activeChatRightDock === "project-files" ? " has-project-files" : ""}${activeChatRightDock === "browser" ? " has-browser" : ""}${browserExpanded ? " is-browser-expanded" : ""}${browserComposerOverlayOpen ? " has-composer-overlay" : ""}${activeChatRightDock === "side-chat" ? " has-side-chat" : ""}${activeChatRightDock === "inspector" ? " has-chat-right" : ""}${activeChatRightDock === "review" ? " has-review" : ""}${hasChatRightDock ? " has-right-dock" : ""}`}
                  style={
                    {
                      "--project-files-current-width": `${projectFilesWidth}px`,
                      "--browser-composer-height": `${browserComposerHeight}px`,
                    } as CSSProperties
                  }
                >
                  <div
                    className={`chat-main${colorStyle === "dynamic" || wallpaperEnabled ? " has-dynamic-palette" : ""}`}
                  >
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
                              void openSessionFromFilespace(
                                chat.sideParentSessionId!,
                              )
                            }
                          >
                            <ArrowLeft size={13} aria-hidden />
                            {t("chat.side.exit")}
                          </button>
                        )}
                      </div>
                    )}
                    <ChatView
                      projectId={activeProjectId}
                      sessionId={chat.sessionId}
                      messages={chat.messages}
                      workspaceContent={
                        projectFiles.tabs.length > 0 ? (
                          <ProjectFileEditor
                            workbench={projectFiles}
                            theme={resolved}
                            mdMode={projectMdMode}
                          />
                        ) : null
                      }
                      composerPresentation={
                        browserExpanded ? "capsule" : "default"
                      }
                      onComposerHeightChange={
                        browserExpanded ? setBrowserComposerHeight : undefined
                      }
                      onComposerOverlayOpenChange={
                        browserExpanded
                          ? setBrowserComposerOverlayOpen
                          : undefined
                      }
                      input={chat.input}
                      composerContextPrefill={composerContextPrefill}
                      onComposerContextPrefillConsumed={() =>
                        setComposerContextPrefill(null)
                      }
                      attachments={chat.attachments}
                      streaming={chat.streaming}
                      turnInFlight={chat.turnInFlight}
                      completionCelebrationId={chat.completionCelebrationId}
                      streamPaused={chat.streamPaused}
                      sendBlocked={chat.isCompacting || chat.sessionReadOnly}
                      sendBlockedReason={
                        chat.isCompacting
                          ? t("chat.compactInProgress")
                          : chat.sessionReadOnly
                            ? chat.sessionEndReason === "compacted" ||
                              !chat.sessionEndReason
                              ? t("chat.sessionCompactedReadOnly")
                              : t("chat.sessionEndedReadOnly")
                            : undefined
                      }
                      displayPrefs={chatDisplayPrefs}
                      onDefaultAnswerLayoutChange={setAnswerLayout}
                      emptyMode={chat.emptyMode}
                      onHeaderUnderlayChange={setChatHeaderHasUnderlay}
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
                      onOpenQueuedFollowUpInNewTask={
                        chat.openQueuedFollowUpInNewTask
                      }
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
                      onOpenFileReview={openFileReview}
                      onEditUserMessage={editUserMessage}
                      onBranchMessage={(id) => void branchMessage(id)}
                      onSlashAction={handleSlashAction}
                      contextUsage={chat.contextUsage}
                      contextWindow={contextWindow}
                      modelId={activeProvider?.model ?? null}
                      realtimeProviderId={activeProvider?.id ?? null}
                      realtimeBackendId={activeProvider?.backend_id ?? null}
                      realtimeAvailable={Boolean(
                        activeProvider?.backend_id === "openai" &&
                          activeProvider.has_api_key,
                      )}
                      cronProviders={providers.map((provider) => ({
                        id: provider.id,
                        name: provider.display_name,
                        model: provider.model,
                        kind: provider.kind,
                      }))}
                      cronActiveProviderId={activeProvider?.id ?? null}
                      modelCapabilities={activeModelCapabilities}
                      modelPricing={activeModelPricing}
                      contextUsagePercent={
                        chat.contextUsage && contextWindow > 0
                          ? usagePercent(
                              chat.contextUsage.totalTokens,
                              contextWindow,
                            )
                          : null
                      }
                    />
                    {terminalDockPresence.mounted &&
                    activeProjectRoot &&
                    activeProject ? (
                      <Suspense fallback={null}>
                        <TerminalDock
                          key={activeProject.id}
                          open={terminalDockPresence.visible}
                          projectId={activeProject.id}
                          projectName={activeProject.name}
                          projectRoot={activeProjectRoot}
                          onClose={() => setTerminalDockOpen(false)}
                        />
                      </Suspense>
                    ) : null}
                    {colorStyle === "dynamic" || wallpaperEnabled ? (
                      <DynamicPaletteButton
                        label={
                          wallpaperEnabled
                            ? t("prefs.wallpaper.cycle")
                            : t("prefs.colorStyle.reshuffle")
                        }
                        onReshuffle={
                          wallpaperEnabled
                            ? () => {
                                if (wallpaper.prefs.recent.length > 1) {
                                  wallpaper.cycleRecent();
                                } else {
                                  openSettingsTab("preferences:appearance");
                                }
                              }
                            : reshuffleDynamic
                        }
                      />
                    ) : null}
                  </div>
                  <ProjectFilesPanel
                    open={activeChatRightDock === "project-files"}
                    workbench={projectFiles}
                    onWidthChange={setProjectFilesWidth}
                  />
                  {browserDockPresence.mounted ? (
                    <BrowserDock
                      open={browserDockPresence.visible}
                      preview={chat.browserPreview}
                      expanded={browserExpanded}
                      onControl={chat.controlBrowser}
                      onExpandedChange={setBrowserExpanded}
                      onTitleMouseDown={winChrome.onTitleMouseDown}
                      onTitleDoubleClick={winChrome.onTitleDoubleClick}
                      onClose={() => {
                        setBrowserExpanded(false);
                        setBrowserComposerOverlayOpen(false);
                        setBrowserDockOpen(false);
                      }}
                    />
                  ) : null}
                  <div
                    className={`side-chat-dock${activeChatRightDock === "side-chat" ? " is-open" : ""}`}
                    aria-hidden={activeChatRightDock !== "side-chat"}
                  >
                    <AnimatePresence initial={false}>
                      {activeChatRightDock === "side-chat" &&
                        sideSessionId &&
                        activeProvider && (
                          <SideChatPanel
                            key={`side-chat-${sideSessionId}`}
                            sessionId={sideSessionId}
                            parentSessionId={sideHostSessionId}
                            activeProjectId={activeProjectId}
                            provider={activeProvider}
                            providers={providers}
                            displayPrefs={chatDisplayPrefs}
                            onDefaultAnswerLayoutChange={setAnswerLayout}
                            interactionMode={chatMode}
                            thinkingPrefs={thinkingPrefs}
                            showThinkingControls={showThinking}
                            reasoningMeta={activeModelReasoning}
                            modelCapabilities={activeModelCapabilities}
                            modelPricing={activeModelPricing}
                            contextWindow={contextWindow}
                            onThinkingLevelChange={onThinkingLevelChange}
                            onToggleThinking={onToggleThinking}
                            onOpenMcpSettings={() => {
                              setSkillsInitialTab("mcp");
                              setNav("skills");
                            }}
                            onOpenContext={() => openChatRightDock("context")}
                            onOpenFileReview={openFileReview}
                            onClose={closeSideChat}
                          />
                        )}
                    </AnimatePresence>
                  </div>
                  <AnimatePresence initial={false}>
                    {activeChatRightDock === "inspector" && (
                      <ChatRightPanel
                        key="chat-inspector"
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
                        subagentRoots={subagents.roots}
                        subagentThreads={subagents.threads}
                        subagentRootServiceTier={
                          subagents.state.rootServiceTier
                        }
                        subagentError={subagents.error}
                        subagentsLoading={subagents.loading}
                        subagentsInitialized={subagents.initialized}
                        onRefreshSubagents={subagents.refresh}
                        onMarkSubagentRead={subagents.markRead}
                        onOpenSession={(sessionId) =>
                          openSessionFromFilespace(sessionId)
                        }
                        onOpenSideSession={(sessionId) => {
                          projectFiles.setPanelOpen(false);
                          setSideSessionId(sessionId);
                          setSideHostSessionId(chat.sessionId);
                          setChatRightOpen(false);
                        }}
                        onPrefillInput={setInput}
                        onOpenMemory={() => openSettingsTab("memory")}
                        onOpenSkills={() => setNav("skills")}
                      />
                    )}
                    {activeChatRightDock === "review" && reviewState && (
                      <ChatReviewPanel
                        key="chat-review"
                        projectId={activeProjectId}
                        files={reviewState.files}
                        selectedPath={reviewState.selectedPath}
                        onSelectPath={(selectedPath) =>
                          setReviewState((current) =>
                            current ? { ...current, selectedPath } : current,
                          )
                        }
                        onClose={() => setReviewState(null)}
                      />
                    )}
                  </AnimatePresence>
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
          projectPath={
            projects.find((p) => p.id === projectMenu.id)?.roots[0] ?? ""
          }
          canRemove={projectMenu.id !== "default"}
          onAction={(action) => {
            if (action === "remove") {
              if (projectMenu.id === "default") return;
              void invoke("delete_project", {
                projectId: projectMenu.id,
              }).catch(() => {});
              setProjects((prev) =>
                prev.filter((p) => p.id !== projectMenu.id),
              );
              if (activeProjectId === projectMenu.id) {
                setActiveProjectId(projects[0]?.id ?? "default");
              }
            } else if (action === "reveal") {
              const root = projects.find((p) => p.id === projectMenu.id)
                ?.roots[0];
              if (root) {
                void import("@tauri-apps/plugin-opener")
                  .then((mod) => mod.revealItemInDir(root))
                  .catch(() => {});
              }
            } else if (action === "pin") {
              void invoke("move_project", {
                projectId: projectMenu.id,
                beforeProjectId: null,
              })
                .then(() =>
                  invoke<ProjectDto[]>("list_projects").then((list) => {
                    if (list) setProjects(list);
                  }),
                )
                .catch(() => {});
            } else if (action === "edit") {
              const proj = projects.find((p) => p.id === projectMenu.id);
              if (proj) setProjectDialog({ mode: "edit", project: proj });
            } else if (action === "worktree") {
              window.alert("功能开发中");
            } else if (action === "archive") {
              void invoke<import("./types").RecentSessionDto[]>(
                "list_sessions",
                {
                  filter: "active",
                  limit: 200,
                  projectId: projectMenu.id,
                },
              )
                .then((sessions) => {
                  if (sessions) {
                    for (const s of sessions) {
                      void invoke("archive_session", {
                        sessionId: s.sessionId,
                      }).catch(() => {});
                    }
                  }
                })
                .catch(() => {});
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
          setProjects((prev) =>
            prev.map((p) => (p.id === updated.id ? updated : p)),
          );
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
