/** 记忆面板：MEMORY/USER/日记编辑与召回。 */
import {
  Fragment,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ArrowLeft,
  Book,
  Check,
  ClipboardList,
  Files,
  Fingerprint,
  List,
  MoonStar,
  Pencil,
  Save,
  Sparkles,
  User,
  Users,
  Wrench,
  X,
  type LucideIcon,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useI18n } from "../../i18n/LocaleContext";
import MotionSwitch from "../ui/MotionSwitch";
import AgentAvatar from "../agents/AgentAvatar";
import { EmptyIllustration } from "../../illustrations";
import type { AgentInfo } from "../../types/agent";
import { buildMonthTimeline, countMonthEntries } from "./memoryTimeline";

/** 记忆面板入参 */
type Props = {
  onClose?: () => void;
  /** 当前聊天会话 id；批准写入后用于刷新活会话 frozen snapshot */
  sessionId?: string | null;
};

/** `config.toml` 记忆开关（Tauri camelCase） */
type MemorySettings = {
  writeApproval: boolean;
  backgroundReviewEnabled: boolean;
  autoRefreshOnUpdate: boolean;
};

/** 单个 Agent 的做梦统计 */
type DreamAgentStatus = {
  agent_id: string;
  agent_name: string;
  points: number;
  new_memories: number;
  pending_diaries: number;
  last_run_at?: string | null;
  last_error?: string | null;
  dreamed_dates?: string[];
};

/** 做梦子系统整体状态 */
type DreamingStatus = {
  enabled: boolean;
  running: boolean;
  last_run_at?: string | null;
  last_error?: string | null;
  total_points: number;
  total_summaries: number;
  pending_diaries: number;
  agents: DreamAgentStatus[];
};

/** 手动触发一轮做梦的结果摘要 */
type DreamRunReport = {
  ok: boolean;
  agents_processed: number;
  diaries_processed: number;
  total_summaries: number;
  total_points: number;
  last_error?: string | null;
};

/** 记忆面板子视图 */
type MemoryView = "diary" | "dream" | "longterm" | "pending";

/** `write_approval` 待审批写入（Tauri camelCase） */
type PendingMemoryWrite = {
  id: string;
  agentId: string;
  target: string;
  action: string;
  content?: string | null;
  oldText?: string | null;
  source: string;
  createdAt: string;
};

/** 长期记忆归档文件 id */
type ArchiveId = "identity" | "user" | "soul" | "agents" | "tools";

const ARCHIVE_FILES: { id: ArchiveId; filename: string; Icon: LucideIcon }[] = [
  { id: "identity", filename: "IDENTITY.md", Icon: Fingerprint },
  { id: "user", filename: "USER.md", Icon: User },
  { id: "soul", filename: "SOUL.md", Icon: Sparkles },
  { id: "agents", filename: "AGENTS.md", Icon: Users },
  { id: "tools", filename: "TOOLS.md", Icon: Wrench },
];

const ALL_AGENTS = "__all__";

/** 本地时区今日 `YYYY-MM-DD` */
function todayLocal(): string {
  const d = new Date();
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** 解析 `YYYY-MM-DD` */
function parseYmd(ymd: string): { y: number; m: number; d: number } {
  const [y, m, d] = ymd.split("-").map(Number);
  return { y, m, d };
}

/** 格式化为 `YYYY-MM-DD` */
function formatYmd(y: number, m: number, d: number): string {
  return `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
}

/** 日期的本地化长标签（含星期） */
function weekdayLabel(ymd: string, locale: string): string {
  const { y, m, d } = parseYmd(ymd);
  const dt = new Date(y, m - 1, d);
  return dt.toLocaleDateString(locale === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "long",
    day: "numeric",
    weekday: "long",
  });
}

/** 时间线中的紧凑日期标签 */
function compactDateLabel(ymd: string, locale: string): string {
  const { y, m, d } = parseYmd(ymd);
  return new Date(y, m - 1, d).toLocaleDateString(
    locale === "zh" ? "zh-CN" : "en-US",
    {
      month: "short",
      day: "numeric",
    },
  );
}

/** 日记视图 */
function IconBook(props: { width?: number; height?: number }) {
  return <Book size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 做梦视图 */
function IconMoon(props: { width?: number; height?: number }) {
  return <MoonStar size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 长期记忆列表 */
function IconList(props: { width?: number; height?: number }) {
  return <List size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 写入审批 */
function IconPending(props: { width?: number; height?: number }) {
  return (
    <ClipboardList size={props.width ?? 16} strokeWidth={1.8} aria-hidden />
  );
}

/** 编辑 */
function IconPencil(props: { width?: number; height?: number }) {
  return <Pencil size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 归档文件 */
function IconFiles(props: { width?: number; height?: number }) {
  return <Files size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 返回 */
function IconArrowLeft(props: { width?: number; height?: number }) {
  return <ArrowLeft size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

/** 保存 */
function IconSave(props: { width?: number; height?: number }) {
  return <Save size={props.width ?? 16} strokeWidth={1.8} aria-hidden />;
}

export default function MemoryPanel({ onClose, sessionId = null }: Props) {
  const { t, locale } = useI18n();
  const confirm = useConfirm();
  const { agents, activeAgentId, setActiveAgent, refreshAgents } =
    useActiveAgent();
  const [view, setView] = useState<MemoryView>("diary");
  const [, setMemoryDir] = useState("");
  const [workspaceDir, setWorkspaceDir] = useState("");
  const [filterAgentId, setFilterAgentId] = useState("default");
  /** 右侧正文当前展示对应的专家（与侧栏选中可短暂不同，避免切换时内容区闪跳） */
  const [diaryPaneAgentId, setDiaryPaneAgentId] = useState(ALL_AGENTS);
  const filterSwitchGen = useRef(0);

  const [calYear, setCalYear] = useState(() => new Date().getFullYear());
  const [calMonth, setCalMonth] = useState(() => new Date().getMonth() + 1);
  const [dailyDate, setDailyDate] = useState(todayLocal());
  const [dailyDates, setDailyDates] = useState<string[]>([]);
  const [allDiaryDates, setAllDiaryDates] = useState<Set<string>>(
    () => new Set(),
  );
  /** agentId → 有日记的日期，用于「全部」模式下点日历跳转 */
  const [diaryDatesByAgent, setDiaryDatesByAgent] = useState<
    Record<string, string[]>
  >({});
  const [dailyDraft, setDailyDraft] = useState("");
  const [dailySaved, setDailySaved] = useState("");

  const [memoryDraft, setMemoryDraft] = useState("");
  const [memorySaved, setMemorySaved] = useState("");
  const [memoryEditing, setMemoryEditing] = useState(false);
  const [showArchives, setShowArchives] = useState(false);
  const [archiveId, setArchiveId] = useState<ArchiveId>("identity");
  const [archiveDraft, setArchiveDraft] = useState("");
  const [archiveSaved, setArchiveSaved] = useState("");

  const [dreamStatus, setDreamStatus] = useState<DreamingStatus | null>(null);
  const [dreamRunning, setDreamRunning] = useState(false);
  const [pendingWrites, setPendingWrites] = useState<PendingMemoryWrite[]>([]);
  const [pendingBusyId, setPendingBusyId] = useState<string | null>(null);
  const [memorySettings, setMemorySettings] = useState<MemorySettings>({
    writeApproval: false,
    backgroundReviewEnabled: false,
    autoRefreshOnUpdate: true,
  });
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const dreamingEnabled = dreamStatus?.enabled ?? false;

  const diaryDirty = dailyDraft !== dailySaved;
  const memoryDirty = memoryDraft !== memorySaved;
  const archiveDirty = archiveDraft !== archiveSaved;
  const dirty =
    view === "diary"
      ? diaryDirty
      : view === "longterm"
        ? showArchives
          ? archiveDirty
          : memoryDirty
        : false;

  const markedDates = useMemo(() => {
    if (filterAgentId === ALL_AGENTS) return allDiaryDates;
    const cached = diaryDatesByAgent[filterAgentId];
    if (cached) return new Set(cached);
    return new Set(dailyDates);
  }, [filterAgentId, allDiaryDates, diaryDatesByAgent, dailyDates]);

  const dreamMarkedDates = useMemo(() => {
    const agentsStatus = dreamStatus?.agents ?? [];
    if (filterAgentId === ALL_AGENTS) {
      const union = new Set<string>();
      for (const a of agentsStatus) {
        for (const d of a.dreamed_dates ?? []) union.add(d);
      }
      return union;
    }
    const mine = agentsStatus.find((a) => a.agent_id === filterAgentId);
    return new Set(mine?.dreamed_dates ?? []);
  }, [dreamStatus, filterAgentId]);

  const monthPrefix = `${calYear}-${String(calMonth).padStart(2, "0")}-`;

  const monthDiaryCount = useMemo(() => {
    if (filterAgentId === ALL_AGENTS) {
      return countMonthEntries(Object.values(diaryDatesByAgent), monthPrefix);
    }
    const dates = diaryDatesByAgent[filterAgentId] ?? dailyDates;
    return countMonthEntries([dates], monthPrefix);
  }, [diaryDatesByAgent, dailyDates, filterAgentId, monthPrefix]);

  const monthDreamCount = useMemo(() => {
    const agentsStatus = dreamStatus?.agents ?? [];
    if (filterAgentId === ALL_AGENTS) {
      return countMonthEntries(
        agentsStatus.map((agent) => agent.dreamed_dates ?? []),
        monthPrefix,
      );
    }
    const mine = agentsStatus.find((agent) => agent.agent_id === filterAgentId);
    return countMonthEntries([mine?.dreamed_dates ?? []], monthPrefix);
  }, [dreamStatus, filterAgentId, monthPrefix]);

  const monthTimeline = useMemo(
    () =>
      buildMonthTimeline({
        monthPrefix,
        diaryDates: markedDates,
        dreamDates: dreamMarkedDates,
        diaryDatesByAgent,
        includeAgentCount: filterAgentId === ALL_AGENTS,
      }),
    [
      diaryDatesByAgent,
      dreamMarkedDates,
      filterAgentId,
      markedDates,
      monthPrefix,
    ],
  );

  const readText = async (path: string) => {
    try {
      return await invoke<string>("read_file", { path });
    } catch {
      return "";
    }
  };

  const refreshDreamStatus = useCallback(async () => {
    try {
      const st = await invoke<DreamingStatus>("get_dreaming_status");
      setDreamStatus(st);
      setDreamRunning(st.running);
    } catch {
      setDreamStatus(null);
    }
  }, []);

  const refreshPendingWrites = useCallback(async () => {
    try {
      const rows = await invoke<PendingMemoryWrite[]>(
        "list_pending_memory_writes",
      );
      setPendingWrites(rows ?? []);
    } catch {
      setPendingWrites([]);
    }
  }, []);

  const refreshMemorySettings = useCallback(async () => {
    try {
      const s = await invoke<MemorySettings>("get_memory_settings");
      setMemorySettings(s);
    } catch {
      setMemorySettings({
        writeApproval: false,
        backgroundReviewEnabled: false,
        autoRefreshOnUpdate: true,
      });
    }
  }, []);

  const setWriteApproval = async (enabled: boolean) => {
    setError(null);
    setSaveMsg(null);
    setSettingsBusy(true);
    try {
      const s = await invoke<MemorySettings>("set_memory_write_approval", {
        enabled,
      });
      setMemorySettings(s);
      setSaveMsg(
        enabled
          ? t("memory.settings.writeApprovalOn")
          : t("memory.settings.writeApprovalOff"),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setSettingsBusy(false);
    }
  };

  const setBackgroundReview = async (enabled: boolean) => {
    setError(null);
    setSaveMsg(null);
    setSettingsBusy(true);
    try {
      const s = await invoke<MemorySettings>("set_background_review_enabled", {
        enabled,
      });
      setMemorySettings(s);
      setSaveMsg(
        enabled
          ? t("memory.settings.backgroundReviewOn")
          : t("memory.settings.backgroundReviewOff"),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setSettingsBusy(false);
    }
  };

  const setAutoRefresh = async (enabled: boolean) => {
    setError(null);
    setSaveMsg(null);
    setSettingsBusy(true);
    try {
      const s = await invoke<MemorySettings>("set_memory_auto_refresh", {
        enabled,
      });
      setMemorySettings(s);
      setSaveMsg(
        enabled
          ? t("memory.settings.autoRefreshOn")
          : t("memory.settings.autoRefreshOff"),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setSettingsBusy(false);
    }
  };

  const refreshIntoChat = async () => {
    setError(null);
    setSaveMsg(null);
    try {
      await invoke("refresh_memory", {
        agentId: filterAgentId === ALL_AGENTS ? null : filterAgentId,
        sessionId: sessionId ?? null,
      });
      setSaveMsg(t("memory.refresh.done"));
    } catch (e) {
      setError(String(e));
    }
  };

  const approvePending = async (id: string) => {
    setError(null);
    setSaveMsg(null);
    setPendingBusyId(id);
    try {
      const msg = await invoke<string>("approve_pending_memory_write", { id });
      setSaveMsg(msg || t("memory.pending.approved"));
      await refreshPendingWrites();
      try {
        await invoke("refresh_memory", {
          agentId: null,
          sessionId: sessionId ?? null,
        });
      } catch {
        // 非阻塞
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setPendingBusyId(null);
    }
  };

  const rejectPending = async (id: string) => {
    setError(null);
    setSaveMsg(null);
    setPendingBusyId(id);
    try {
      await invoke("reject_pending_memory_write", { id });
      setSaveMsg(t("memory.pending.rejected"));
      await refreshPendingWrites();
    } catch (e) {
      setError(String(e));
    } finally {
      setPendingBusyId(null);
    }
  };

  const enableDreaming = async () => {
    setError(null);
    setSaveMsg(null);
    try {
      const st = await invoke<DreamingStatus>("set_dreaming_enabled_cmd", {
        enabled: true,
      });
      setDreamStatus(st);
      setSaveMsg(t("memory.dream.enabledHint"));
    } catch (e) {
      setError(String(e));
    }
  };

  const disableDreaming = async () => {
    setError(null);
    setSaveMsg(null);
    try {
      const st = await invoke<DreamingStatus>("set_dreaming_enabled_cmd", {
        enabled: false,
      });
      setDreamStatus(st);
    } catch (e) {
      setError(String(e));
    }
  };

  const runDreamingAgain = async () => {
    setError(null);
    setSaveMsg(null);
    setDreamRunning(true);
    try {
      const report = await invoke<DreamRunReport>("run_dreaming");
      await refreshDreamStatus();
      if (report.diaries_processed === 0 && report.ok) {
        setSaveMsg(t("memory.dream.nothingToDo"));
      } else if (report.ok) {
        setSaveMsg(
          t("memory.dream.runOk", {
            agents: String(report.agents_processed),
            diaries: String(report.diaries_processed),
          }),
        );
      } else {
        setError(report.last_error || t("memory.dream.runPartial"));
      }
    } catch (e) {
      setError(String(e));
      await refreshDreamStatus();
    } finally {
      setDreamRunning(false);
    }
  };

  const refreshAllDiaryMarks = useCallback(async (list: AgentInfo[]) => {
    const pairs = await Promise.all(
      list.map(async (a) => {
        try {
          const dates = await invoke<string[]>("list_daily_memory", {
            agentId: a.id,
          });
          return [a.id, dates] as const;
        } catch {
          return [a.id, [] as string[]] as const;
        }
      }),
    );
    const byAgent: Record<string, string[]> = {};
    const union = new Set<string>();
    for (const [id, dates] of pairs) {
      byAgent[id] = dates;
      for (const d of dates) union.add(d);
    }
    setDiaryDatesByAgent(byAgent);
    setAllDiaryDates(union);
  }, []);

  const loadDaily = useCallback(async (agentId: string, date: string) => {
    const dates = await invoke<string[]>("list_daily_memory", { agentId });
    setDailyDates(dates);
    const content = await invoke<string>("read_daily_memory", {
      date,
      agentId,
    });
    setDailyDraft(content);
    setDailySaved(content);
  }, []);

  const loadMemoryMd = useCallback(async (dir: string) => {
    const content = await readText(`${dir}/MEMORY.md`);
    setMemoryDraft(content);
    setMemorySaved(content);
  }, []);

  const loadArchive = useCallback(async (dir: string, id: ArchiveId) => {
    const file = ARCHIVE_FILES.find((f) => f.id === id)!;
    const content = await readText(`${dir}/${file.filename}`);
    setArchiveDraft(content);
    setArchiveSaved(content);
  }, []);

  const applyConfig = useCallback(
    async (cfg: {
      memory_dir?: string;
      workspace_dir: string;
      active_agent_id: string;
      agents: AgentInfo[];
    }) => {
      if (cfg.memory_dir) setMemoryDir(cfg.memory_dir);
      setWorkspaceDir(cfg.workspace_dir);
      await refreshAgents();
      let nextFilter = filterAgentId;
      if (
        filterAgentId !== ALL_AGENTS &&
        !cfg.agents.some((a) => a.id === filterAgentId)
      ) {
        nextFilter = ALL_AGENTS;
        setFilterAgentId(ALL_AGENTS);
        setDiaryPaneAgentId(ALL_AGENTS);
      }
      const date = dailyDate || todayLocal();
      if (nextFilter === ALL_AGENTS) {
        setDailyDraft("");
        setDailySaved("");
        setDiaryPaneAgentId(ALL_AGENTS);
      } else {
        await loadDaily(nextFilter, date);
        setDiaryPaneAgentId(nextFilter);
      }
      await loadMemoryMd(cfg.workspace_dir);
      await loadArchive(cfg.workspace_dir, archiveId);
      await refreshAllDiaryMarks(cfg.agents);
    },
    [
      archiveId,
      dailyDate,
      filterAgentId,
      loadArchive,
      loadDaily,
      loadMemoryMd,
      refreshAllDiaryMarks,
      refreshAgents,
    ],
  );

  const bootstrap = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const cfg = await invoke<{
        memory_dir: string;
        workspace_dir: string;
        active_agent_id: string;
        agents: AgentInfo[];
      }>("get_config");
      setMemoryDir(cfg.memory_dir);
      await refreshDreamStatus();
      await refreshMemorySettings();
      await applyConfig(cfg);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [applyConfig, refreshDreamStatus, refreshMemorySettings]);

  useEffect(() => {
    void bootstrap();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount once
  }, []);

  useEffect(() => {
    if (view !== "dream") return;
    void refreshDreamStatus();
  }, [view, refreshDreamStatus]);

  useEffect(() => {
    if (view !== "pending") return;
    void refreshPendingWrites();
    void refreshMemorySettings();
  }, [view, refreshPendingWrites, refreshMemorySettings]);

  const confirmIfDirty = async () => {
    if (!dirty) return true;
    return await confirm({
      title: t("dialog.unsavedTitle"),
      message: t("memory.unsavedConfirm"),
    });
  };

  const switchView = async (next: MemoryView) => {
    if (next === view) return;
    if (!(await confirmIfDirty())) return;
    setView(next);
    setSaveMsg(null);
    setError(null);
    setShowArchives(false);
    if (next === "longterm" && filterAgentId === ALL_AGENTS) {
      const target = activeAgentId || agents[0]?.id;
      if (target) {
        setFilterAgentId(target);
        void (async () => {
          setLoading(true);
          try {
            await setActiveAgent(target);
            const cfg = await invoke<{
              memory_dir: string;
              workspace_dir: string;
            }>("get_config");
            setMemoryDir(cfg.memory_dir);
            setWorkspaceDir(cfg.workspace_dir);
            await loadMemoryMd(cfg.workspace_dir);
            await loadArchive(cfg.workspace_dir, archiveId);
          } catch (e) {
            setError(String(e));
          } finally {
            setLoading(false);
          }
        })();
      }
    }
  };

  const switchFilterAgent = async (agentId: string, forDate?: string) => {
    if (agentId === filterAgentId && forDate == null) return;
    if (!(await confirmIfDirty())) return;
    const date = forDate ?? dailyDate;
    const gen = ++filterSwitchGen.current;
    setFilterAgentId(agentId);
    if (forDate) setDailyDate(forDate);
    setSaveMsg(null);
    setError(null);
    try {
      if (agentId === ALL_AGENTS) {
        // 汇总浏览：立即清空正文，避免残留某专家草稿造成空态错乱
        setDiaryPaneAgentId(ALL_AGENTS);
        setDailyDraft("");
        setDailySaved("");
        await refreshAllDiaryMarks(agents);
      } else {
        // 侧栏先切高亮；正文等加载完成再切，避免空态/编辑器来回闪
        const cached = diaryDatesByAgent[agentId];
        if (cached) setDailyDates(cached);
        await setActiveAgent(agentId);
        if (gen !== filterSwitchGen.current) return;
        const cfg = await invoke<{
          memory_dir: string;
          workspace_dir: string;
          agents: AgentInfo[];
        }>("get_config");
        if (gen !== filterSwitchGen.current) return;
        setMemoryDir(cfg.memory_dir);
        setWorkspaceDir(cfg.workspace_dir);
        await loadDaily(agentId, date);
        if (gen !== filterSwitchGen.current) return;
        setDiaryPaneAgentId(agentId);
        if (view !== "diary") {
          await loadMemoryMd(cfg.workspace_dir);
          await loadArchive(cfg.workspace_dir, archiveId);
        }
        await refreshAllDiaryMarks(cfg.agents);
      }
    } catch (e) {
      if (gen === filterSwitchGen.current) setError(String(e));
    }
  };

  const preferAgentForDate = useCallback(
    (ymd: string): string | null => {
      const idsWithDiary = agents
        .map((a) => a.id)
        .filter((id) => (diaryDatesByAgent[id] ?? []).includes(ymd));
      if (!idsWithDiary.length) return null;
      if (idsWithDiary.includes(activeAgentId)) return activeAgentId;
      const def = agents.find((a) => a.is_default)?.id;
      if (def && idsWithDiary.includes(def)) return def;
      return idsWithDiary[0] ?? null;
    },
    [agents, diaryDatesByAgent, activeAgentId],
  );

  const switchDailyDate = async (date: string) => {
    if (date === dailyDate && filterAgentId !== ALL_AGENTS) return;
    if (
      diaryDirty &&
      !(await confirm({
        title: t("dialog.unsavedTitle"),
        message: t("memory.unsavedConfirm"),
      }))
    ) {
      return;
    }
    setSaveMsg(null);
    setError(null);

    if (filterAgentId === ALL_AGENTS) {
      const target = preferAgentForDate(date);
      if (target) {
        await switchFilterAgent(target, date);
        return;
      }
      setDailyDate(date);
      setDailyDraft("");
      setDailySaved("");
      setSaveMsg(t("memory.diaryNoAgentOnDate"));
      return;
    }

    if (date === dailyDate) return;
    setDailyDate(date);
    try {
      await loadDaily(filterAgentId, date);
    } catch (e) {
      setError(String(e));
    }
  };

  const shiftMonth = (delta: number) => {
    let m = calMonth + delta;
    let y = calYear;
    if (m < 1) {
      m = 12;
      y -= 1;
    } else if (m > 12) {
      m = 1;
      y += 1;
    }
    setCalYear(y);
    setCalMonth(m);
  };

  const saveDiary = async () => {
    const agentId =
      filterAgentId === ALL_AGENTS ? activeAgentId : filterAgentId;
    setSaving(true);
    setError(null);
    try {
      await invoke("write_daily_memory", {
        content: dailyDraft,
        date: dailyDate,
        agentId,
      });
      setDailySaved(dailyDraft);
      const dates = await invoke<string[]>("list_daily_memory", { agentId });
      setDailyDates(dates);
      await refreshAllDiaryMarks(agents);
      setSaveMsg(t("memory.saved"));
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const saveLongterm = async () => {
    if (!workspaceDir) return;
    setSaving(true);
    setError(null);
    try {
      if (showArchives) {
        const file = ARCHIVE_FILES.find((f) => f.id === archiveId)!;
        await invoke("write_file", {
          path: `${workspaceDir}/${file.filename}`,
          content: archiveDraft,
        });
        setArchiveSaved(archiveDraft);
      } else {
        await invoke("write_file", {
          path: `${workspaceDir}/MEMORY.md`,
          content: memoryDraft,
        });
        setMemorySaved(memoryDraft);
      }
      setSaveMsg(t("memory.saved"));
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const calendarCells = useMemo(() => {
    const first = new Date(calYear, calMonth - 1, 1);
    const startPad = (first.getDay() + 6) % 7; // Monday-first
    const daysInMonth = new Date(calYear, calMonth, 0).getDate();
    const cells: ({ day: number; ymd: string } | null)[] = [];
    for (let i = 0; i < startPad; i++) cells.push(null);
    for (let d = 1; d <= daysInMonth; d++) {
      cells.push({ day: d, ymd: formatYmd(calYear, calMonth, d) });
    }
    while (cells.length % 7 !== 0) cells.push(null);
    return cells;
  }, [calYear, calMonth]);

  const weekdays =
    locale === "zh"
      ? ["一", "二", "三", "四", "五", "六", "日"]
      : ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

  const diaryEmpty = !dailyDraft.trim();

  return (
    <aside className="side-panel memory-panel">
      <div className="mem-top">
        <h2 className="mem-top-title">{t("page.memory.title")}</h2>

        <nav className="mem-view-nav" aria-label={t("memory.views")}>
          <div className="mem-view-pipeline">
            <span className="mem-view-flow" aria-hidden>
              <span className="mem-view-flow-ember mem-view-flow-ember--src" />
              <span className="mem-view-flow-ember mem-view-flow-ember--dst" />
              {Array.from({ length: 10 }, (_, i) => (
                <span
                  key={i}
                  className={`mem-view-flow-pulse mem-view-flow-pulse--${i % 5}`}
                  style={{ animationDelay: `${(i * 0.42).toFixed(2)}s` }}
                />
              ))}
            </span>
            {(
              [
                {
                  id: "diary" as const,
                  label: t("memory.view.diary"),
                  Icon: IconBook,
                },
                {
                  id: "dream" as const,
                  label: t("memory.view.dream"),
                  Icon: IconMoon,
                },
                {
                  id: "longterm" as const,
                  label: t("memory.view.longterm"),
                  Icon: IconList,
                },
              ] as const
            ).map((item, index) => (
              <Fragment key={item.id}>
                {index > 0 && (
                  <span className="mem-view-step" aria-hidden>
                    ›
                  </span>
                )}
                <button
                  type="button"
                  className={`mem-view-tab mem-view-tab--${item.id} ${view === item.id ? "active" : ""}`}
                  title={item.label}
                  aria-label={item.label}
                  onClick={() => void switchView(item.id)}
                >
                  <item.Icon />
                  <span className="mem-view-tab-label">{item.label}</span>
                </button>
              </Fragment>
            ))}
          </div>
          <span className="mem-view-rail" aria-hidden />
          <button
            type="button"
            className={`mem-view-tab mem-view-tab--pending ${view === "pending" ? "active" : ""}`}
            title={
              pendingWrites.length > 0
                ? `${t("memory.view.pending")} (${pendingWrites.length})`
                : t("memory.view.pending")
            }
            aria-label={
              pendingWrites.length > 0
                ? `${t("memory.view.pending")} (${pendingWrites.length})`
                : t("memory.view.pending")
            }
            onClick={() => void switchView("pending")}
          >
            <IconPending />
            <span className="mem-view-tab-label">
              {t("memory.view.pending")}
              {pendingWrites.length > 0 ? ` (${pendingWrites.length})` : ""}
            </span>
          </button>
        </nav>

        <div className="mem-top-stats">
          {view === "diary" && (
            <>
              <div className="mem-stat" title={t("memory.diaryThisMonth")}>
                <span className="mem-stat-icon" aria-hidden>
                  <IconBook width={14} height={14} />
                </span>
                <span className="mem-stat-body">
                  <strong>{monthDiaryCount}</strong>
                  <span className="mem-stat-label">
                    {t("memory.diaryThisMonth")}
                  </span>
                </span>
              </div>
              <label
                className="mem-enhance"
                data-tip={t("memory.enhanceDiaryTip")}
              >
                <span className="mem-enhance-label">
                  {t("memory.enhanceDiary")}
                </span>
                <button
                  type="button"
                  role="switch"
                  className="prefs-switch"
                  aria-checked={dreamingEnabled}
                  aria-label={t("memory.enhanceDiary")}
                  onClick={() => {
                    void (dreamingEnabled
                      ? disableDreaming()
                      : enableDreaming());
                  }}
                >
                  <span className="prefs-switch-thumb" />
                </button>
              </label>
            </>
          )}
        </div>
      </div>

      {error && <div className="side-error">{error}</div>}
      {saveMsg && <div className="memory-save-msg">{saveMsg}</div>}
      {loading && (
        <div className="mem-loading muted" aria-live="polite">
          {t("memory.loading")}
        </div>
      )}

      <MotionSwitch switchKey={view} className="anim-switch--fill">
        {view === "diary" && (
          <div className="mem-split">
            <aside className="mem-sidebar">
              <section className="mem-card mem-calendar-card">
                <div className="mem-cal-header">
                  <button
                    type="button"
                    className="ws-tool-btn"
                    onClick={() => shiftMonth(-1)}
                    aria-label={t("memory.prevMonth")}
                  >
                    ‹
                  </button>
                  <div className="mem-cal-title">
                    {locale === "zh"
                      ? `${calYear}年${calMonth}月`
                      : new Date(calYear, calMonth - 1).toLocaleDateString(
                          "en-US",
                          {
                            month: "long",
                            year: "numeric",
                          },
                        )}
                  </div>
                  <button
                    type="button"
                    className="ws-tool-btn"
                    onClick={() => shiftMonth(1)}
                    aria-label={t("memory.nextMonth")}
                  >
                    ›
                  </button>
                </div>
                <div className="mem-cal-legend">
                  <span>
                    <i className="mem-dot diary" />
                    {t("memory.hasDiary")}
                  </span>
                  <span>
                    <i className="mem-dot dream" />
                    {t("memory.hasDream")}
                  </span>
                </div>
                <div className="mem-cal-weekdays">
                  {weekdays.map((w) => (
                    <span key={w}>{w}</span>
                  ))}
                </div>
                <div className="mem-cal-grid">
                  {calendarCells.map((cell, i) =>
                    cell ? (
                      <button
                        key={cell.ymd}
                        type="button"
                        className={[
                          "mem-cal-day",
                          cell.ymd === dailyDate ? "selected" : "",
                          markedDates.has(cell.ymd) ? "has-diary" : "",
                          dreamMarkedDates.has(cell.ymd) ? "has-dream" : "",
                          cell.ymd === todayLocal() ? "today" : "",
                        ]
                          .filter(Boolean)
                          .join(" ")}
                        onClick={() => void switchDailyDate(cell.ymd)}
                      >
                        {cell.day}
                      </button>
                    ) : (
                      <span key={`e-${i}`} className="mem-cal-day empty" />
                    ),
                  )}
                </div>
              </section>

              <section
                className="mem-timeline"
                aria-labelledby="mem-timeline-title"
              >
                <div className="mem-timeline-header">
                  <h3 id="mem-timeline-title">{t("memory.timeline.title")}</h3>
                  <span>
                    {t("memory.timeline.summary", {
                      diaries: String(monthDiaryCount),
                      dreams: String(monthDreamCount),
                    })}
                  </span>
                </div>

                {monthTimeline.length > 0 ? (
                  <ol className="mem-timeline-list">
                    {monthTimeline.map((item) => {
                      const selected = item.date === dailyDate;
                      const isToday = item.date === todayLocal();
                      return (
                        <li key={item.date} className="mem-timeline-item">
                          <button
                            type="button"
                            className={selected ? "is-selected" : undefined}
                            aria-current={selected ? "date" : undefined}
                            onClick={() => void switchDailyDate(item.date)}
                          >
                            <span
                              className="mem-timeline-node"
                              data-diary={item.hasDiary || undefined}
                              data-dream={item.hasDream || undefined}
                              aria-hidden
                            />
                            <span className="mem-timeline-content">
                              <span className="mem-timeline-date">
                                {compactDateLabel(item.date, locale)}
                                {isToday && (
                                  <small>{t("memory.timeline.today")}</small>
                                )}
                              </span>
                              <span className="mem-timeline-meta">
                                {item.hasDiary && (
                                  <span>{t("memory.timeline.diary")}</span>
                                )}
                                {item.hasDream && (
                                  <span>{t("memory.timeline.dream")}</span>
                                )}
                                {filterAgentId === ALL_AGENTS &&
                                  item.diaryAgentCount > 0 && (
                                    <span>
                                      {t("memory.timeline.expertCount", {
                                        count: String(item.diaryAgentCount),
                                      })}
                                    </span>
                                  )}
                              </span>
                            </span>
                          </button>
                        </li>
                      );
                    })}
                  </ol>
                ) : (
                  <p className="mem-timeline-empty">
                    {t("memory.timeline.empty")}
                  </p>
                )}
              </section>
            </aside>

            <section className="mem-main mem-card">
              <div className="mem-main-header">
                <h3>{weekdayLabel(dailyDate, locale)}</h3>
                {diaryDirty && (
                  <div className="memory-editor-actions">
                    <button
                      type="button"
                      className="ghost-btn"
                      onClick={() => setDailyDraft(dailySaved)}
                      disabled={saving}
                      data-tip={t("memory.undo")}
                    >
                      {t("memory.undo")}
                    </button>
                    <button
                      type="button"
                      className="ghost-btn active"
                      onClick={() => void saveDiary()}
                      disabled={saving || filterAgentId === ALL_AGENTS}
                      data-tip={t("memory.save")}
                    >
                      {saving ? "…" : t("memory.save")}
                    </button>
                  </div>
                )}
              </div>

              {diaryPaneAgentId === ALL_AGENTS ? (
                <div className="mem-empty mem-empty-pick">
                  <EmptyIllustration
                    scene="memory"
                    size="lg"
                    className="mem-empty-illust"
                    title={t("memory.pickExpertForDiaryAll")}
                    hint={t("memory.pickExpertHintDiaryAll")}
                  />
                </div>
              ) : diaryEmpty ? (
                <div className="mem-empty mem-empty-diary">
                  <EmptyIllustration
                    scene="memory"
                    size="lg"
                    className="mem-empty-illust"
                    title={t("memory.diaryEmptyTitle")}
                    hint={t("memory.diaryEmptySub")}
                  />
                  {onClose && (
                    <button type="button" className="mem-cta" onClick={onClose}>
                      {t("memory.goChat")}
                    </button>
                  )}
                </div>
              ) : (
                <textarea
                  className="memory-editor mem-editor-fill"
                  value={dailyDraft}
                  onChange={(e) => {
                    setDailyDraft(e.target.value);
                    setSaveMsg(null);
                  }}
                  placeholder={t("memory.dailyEmptyHint")}
                  spellCheck={false}
                  aria-label={`memory/${dailyDate}.md`}
                />
              )}
            </section>
          </div>
        )}

        {view === "dream" && (
          <div className="mem-dream">
            {!dreamingEnabled ? (
              <section className="mem-card mem-dream-empty">
                <div className="mem-dream-moon" aria-hidden>
                  <MoonStar size={40} strokeWidth={1.6} />
                </div>
                <h3>{t("memory.dream.enableTitle")}</h3>
                <p className="muted">{t("memory.dream.enableSub")}</p>
                <button
                  type="button"
                  className="mem-cta"
                  disabled={dreamRunning}
                  onClick={() => void enableDreaming()}
                >
                  {t("memory.dream.enable")}
                </button>
              </section>
            ) : (
              <>
                <section className="mem-card mem-dream-banner">
                  <div className="mem-dream-banner-text">
                    <strong>
                      {dreamRunning
                        ? t("memory.dream.running")
                        : (dreamStatus?.pending_diaries ?? 0) > 0
                          ? t("memory.dream.pending", {
                              count: String(dreamStatus?.pending_diaries ?? 0),
                            })
                          : t("memory.dream.idle")}
                    </strong>
                    <span className="muted">
                      {t("memory.dream.runningSub")}
                    </span>
                    {dreamStatus?.last_error && (
                      <span className="mem-dream-error">
                        {dreamStatus.last_error}
                      </span>
                    )}
                  </div>
                  <div className="mem-dream-banner-stats">
                    <span>
                      <strong>{dreamStatus?.total_points ?? 0}</strong>{" "}
                      {t("memory.dream.points")}
                    </span>
                    <span>
                      <strong>{dreamStatus?.total_summaries ?? 0}</strong>{" "}
                      {t("memory.dream.summaries")}
                    </span>
                  </div>
                  <div className="mem-dream-banner-actions">
                    <button
                      type="button"
                      className="ghost-btn active"
                      disabled={dreamRunning}
                      onClick={() => void runDreamingAgain()}
                      data-tip={
                        dreamRunning
                          ? t("memory.dream.runningShort")
                          : t("memory.dream.runNow")
                      }
                    >
                      {dreamRunning
                        ? t("memory.dream.runningShort")
                        : t("memory.dream.runNow")}
                    </button>
                    <button
                      type="button"
                      className="ghost-btn"
                      disabled={dreamRunning}
                      onClick={() => void disableDreaming()}
                      data-tip={t("memory.dream.disable")}
                    >
                      {t("memory.dream.disable")}
                    </button>
                  </div>
                </section>
                <div className="mem-dream-grid">
                  {(dreamStatus?.agents?.length
                    ? dreamStatus.agents
                    : agents.map((a) => ({
                        agent_id: a.id,
                        agent_name: a.name,
                        points: 0,
                        new_memories: 0,
                        pending_diaries: 0,
                        last_run_at: null,
                        last_error: null,
                      }))
                  ).map((a) => (
                    <article
                      key={a.agent_id}
                      className="mem-card mem-dream-agent-card"
                    >
                      <div className="mem-dream-agent-head">
                        <span className="mem-agent-avatar lg" aria-hidden>
                          <AgentAvatar
                            agent={
                              agents.find((x) => x.id === a.agent_id) ?? {
                                id: a.agent_id,
                                name: a.agent_name,
                                is_default: a.agent_id === "default",
                              }
                            }
                            size={36}
                          />
                        </span>
                        <div>
                          <div className="mem-dream-agent-name">
                            {a.agent_name}
                          </div>
                          <div className="muted mem-dream-agent-desc">
                            {a.pending_diaries > 0
                              ? t("memory.dream.agentPending", {
                                  count: String(a.pending_diaries),
                                })
                              : t("memory.dream.agentDesc")}
                          </div>
                          {a.last_error && (
                            <div className="mem-dream-error">
                              {a.last_error}
                            </div>
                          )}
                        </div>
                      </div>
                      <div className="mem-dream-agent-foot">
                        <span>
                          {t("memory.dream.points")} <strong>{a.points}</strong>
                        </span>
                        <span>
                          {t("memory.dream.newMemories")}{" "}
                          <strong>{a.new_memories}</strong>
                        </span>
                      </div>
                    </article>
                  ))}
                </div>
              </>
            )}
          </div>
        )}

        {view === "longterm" && (
          <div className="mem-longterm">
            <section className="mem-card mem-main">
              <div className="mem-main-header">
                <h3>
                  {showArchives
                    ? t("memory.archives")
                    : t("memory.view.longterm")}
                </h3>
                <div className="memory-editor-actions">
                  <button
                    type="button"
                    className={`mem-glass-btn ${showArchives ? "is-active" : ""}`}
                    onClick={() => {
                      setShowArchives((v) => !v);
                      setSaveMsg(null);
                    }}
                    title={
                      showArchives
                        ? t("memory.backToMemory")
                        : t("memory.moreArchives")
                    }
                    aria-label={
                      showArchives
                        ? t("memory.backToMemory")
                        : t("memory.moreArchives")
                    }
                  >
                    {showArchives ? <IconArrowLeft /> : <IconFiles />}
                    <span>
                      {showArchives
                        ? t("memory.backToMemory")
                        : t("memory.moreArchives")}
                    </span>
                  </button>
                  <button
                    type="button"
                    className={`mem-glass-btn ${memoryEditing ? "is-active" : ""}`}
                    onClick={() => setMemoryEditing((v) => !v)}
                    title={t("memory.edit")}
                    aria-label={t("memory.edit")}
                    aria-pressed={memoryEditing}
                  >
                    <IconPencil />
                    <span>{t("memory.edit")}</span>
                  </button>
                  {(showArchives ? archiveDirty : memoryDirty) && (
                    <button
                      type="button"
                      className="mem-glass-btn is-primary"
                      onClick={() => void saveLongterm()}
                      disabled={saving || filterAgentId === ALL_AGENTS}
                      title={t("memory.save")}
                    >
                      <IconSave />
                      <span>{saving ? "…" : t("memory.save")}</span>
                    </button>
                  )}
                </div>
              </div>

              {filterAgentId === ALL_AGENTS ? (
                <div className="mem-empty mem-empty-pick">
                  <EmptyIllustration
                    scene="memory"
                    title={t("memory.pickExpertForMemory")}
                    hint={t("memory.pickExpertHint")}
                  />
                </div>
              ) : showArchives ? (
                <>
                  <div className="memory-file-tabs" role="tablist">
                    {ARCHIVE_FILES.map((f) => {
                      const { Icon } = f;
                      return (
                        <button
                          key={f.id}
                          type="button"
                          role="tab"
                          className={`memory-file-tab ${archiveId === f.id ? "active" : ""}`}
                          onClick={async () => {
                            if (
                              archiveDirty &&
                              !(await confirm({
                                title: t("dialog.unsavedTitle"),
                                message: t("memory.unsavedConfirm"),
                              }))
                            ) {
                              return;
                            }
                            setArchiveId(f.id);
                            void loadArchive(workspaceDir, f.id);
                          }}
                        >
                          <Icon size={13} strokeWidth={2.1} aria-hidden />
                          <span>{f.filename}</span>
                        </button>
                      );
                    })}
                  </div>
                  <textarea
                    className="memory-editor mem-editor-fill"
                    value={archiveDraft}
                    onChange={(e) => {
                      setArchiveDraft(e.target.value);
                      setSaveMsg(null);
                    }}
                    readOnly={!memoryEditing}
                    placeholder={t("memory.emptyHint")}
                    spellCheck={false}
                  />
                </>
              ) : (
                <textarea
                  className="memory-editor mem-editor-fill"
                  value={memoryDraft}
                  onChange={(e) => {
                    setMemoryDraft(e.target.value);
                    setSaveMsg(null);
                  }}
                  readOnly={!memoryEditing}
                  placeholder={t("memory.longtermEmptyHint")}
                  spellCheck={false}
                  aria-label="MEMORY.md"
                />
              )}
            </section>
          </div>
        )}

        {view === "pending" && (
          <div className="mem-pending-wrap">
            <section className="mem-card mem-main mem-pending-settings">
              <div className="mem-main-header">
                <h3>{t("memory.settings.title")}</h3>
                <button
                  type="button"
                  className="mem-glass-btn"
                  onClick={() => void refreshIntoChat()}
                  title={t("memory.refresh.intoChat")}
                >
                  {t("memory.refresh.intoChat")}
                </button>
              </div>
              <div
                className="mem-settings-list"
                role="group"
                aria-label={t("memory.settings.title")}
              >
                <label className="mem-settings-row">
                  <span className="mem-settings-text">
                    <span className="mem-settings-label">
                      {t("memory.settings.writeApproval")}
                    </span>
                    <span className="mem-settings-desc">
                      {t("memory.settings.writeApprovalDesc")}
                    </span>
                  </span>
                  <button
                    type="button"
                    role="switch"
                    className="prefs-switch"
                    aria-checked={memorySettings.writeApproval}
                    aria-label={t("memory.settings.writeApproval")}
                    disabled={settingsBusy}
                    onClick={() =>
                      void setWriteApproval(!memorySettings.writeApproval)
                    }
                  >
                    <span className="prefs-switch-thumb" />
                  </button>
                </label>
                <label className="mem-settings-row">
                  <span className="mem-settings-text">
                    <span className="mem-settings-label">
                      {t("memory.settings.backgroundReview")}
                    </span>
                    <span className="mem-settings-desc">
                      {t("memory.settings.backgroundReviewDesc")}
                    </span>
                  </span>
                  <button
                    type="button"
                    role="switch"
                    className="prefs-switch"
                    aria-checked={memorySettings.backgroundReviewEnabled}
                    aria-label={t("memory.settings.backgroundReview")}
                    disabled={settingsBusy}
                    onClick={() =>
                      void setBackgroundReview(
                        !memorySettings.backgroundReviewEnabled,
                      )
                    }
                  >
                    <span className="prefs-switch-thumb" />
                  </button>
                </label>
                <label className="mem-settings-row">
                  <span className="mem-settings-text">
                    <span className="mem-settings-label">
                      {t("memory.settings.autoRefresh")}
                    </span>
                    <span className="mem-settings-desc">
                      {t("memory.settings.autoRefreshDesc")}
                    </span>
                  </span>
                  <button
                    type="button"
                    role="switch"
                    className="prefs-switch"
                    aria-checked={memorySettings.autoRefreshOnUpdate}
                    aria-label={t("memory.settings.autoRefresh")}
                    disabled={settingsBusy}
                    onClick={() =>
                      void setAutoRefresh(!memorySettings.autoRefreshOnUpdate)
                    }
                  >
                    <span className="prefs-switch-thumb" />
                  </button>
                </label>
              </div>
            </section>
            <section className="mem-card mem-main mem-pending">
              <div className="mem-main-header">
                <h3>{t("memory.view.pending")}</h3>
                <button
                  type="button"
                  className="mem-glass-btn"
                  onClick={() => void refreshPendingWrites()}
                  title={t("memory.pending.refresh")}
                >
                  {t("memory.pending.refresh")}
                </button>
              </div>
              <p className="mem-pending-hint">{t("memory.pending.hint")}</p>
              {pendingWrites.length === 0 ? (
                <div className="mem-empty">
                  <EmptyIllustration
                    scene="memory"
                    size="sm"
                    title={t("memory.pending.emptyTitle")}
                    hint={
                      memorySettings.writeApproval
                        ? t("memory.pending.emptyHintOn")
                        : t("memory.pending.emptyHint")
                    }
                  />
                </div>
              ) : (
                <ul className="mem-pending-list">
                  {pendingWrites.map((p) => (
                    <li key={p.id} className="mem-pending-item">
                      <div className="mem-pending-meta">
                        <span className="mem-pending-badge">{p.action}</span>
                        <span className="mem-pending-badge soft">
                          {p.target}
                        </span>
                        <span className="mem-pending-badge soft">
                          {p.source}
                        </span>
                        <span className="mem-pending-agent">{p.agentId}</span>
                        <span className="mem-pending-time">{p.createdAt}</span>
                      </div>
                      {p.content ? (
                        <pre className="mem-pending-body">{p.content}</pre>
                      ) : null}
                      {p.oldText ? (
                        <pre className="mem-pending-body muted">
                          {p.oldText}
                        </pre>
                      ) : null}
                      <div className="mem-pending-actions">
                        <button
                          type="button"
                          className="mem-glass-btn is-primary"
                          disabled={pendingBusyId === p.id}
                          onClick={() => void approvePending(p.id)}
                        >
                          <Check size={14} strokeWidth={2} aria-hidden />
                          {t("memory.pending.approve")}
                        </button>
                        <button
                          type="button"
                          className="mem-glass-btn"
                          disabled={pendingBusyId === p.id}
                          onClick={() => void rejectPending(p.id)}
                        >
                          <X size={14} strokeWidth={2} aria-hidden />
                          {t("memory.pending.reject")}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </div>
        )}
      </MotionSwitch>
    </aside>
  );
}
