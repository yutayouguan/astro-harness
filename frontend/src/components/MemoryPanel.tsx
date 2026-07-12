/** 记忆面板：MEMORY/USER/日记编辑与召回。 */
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import {
  ArrowLeft,
  Book,
  Files,
  List,
  MoonStar,
  Pencil,
  Save,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import { useAgentsChanged } from "../lib/agentsChanged";
import AnimatedSwitch from "./AnimatedSwitch";
import { IconWsBackChat } from "./WorkspaceIcons";

/** 记忆面板入参 */
type Props = {
  onClose?: () => void;
};

/** 列表用 Agent 简项 */
type AgentInfo = {
  id: string;
  name: string;
  path: string;
  is_default: boolean;
  is_active: boolean;
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
type MemoryView = "diary" | "dream" | "longterm";

/** 长期记忆归档文件 id */
type ArchiveId = "agent" | "identity" | "user" | "soul" | "agents" | "tools";

const ARCHIVE_FILES: { id: ArchiveId; filename: string }[] = [
  { id: "agent", filename: "AGENT.md" },
  { id: "identity", filename: "IDENTITY.md" },
  { id: "user", filename: "USER.md" },
  { id: "soul", filename: "SOUL.md" },
  { id: "agents", filename: "AGENTS.md" },
  { id: "tools", filename: "TOOLS.md" },
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

/** 日记空状态插画 */
function IconDiaryEmpty(props: { width?: number; height?: number }) {
  const w = props.width ?? 88;
  const h = props.height ?? 88;
  return (
    <svg
      viewBox="0 0 96 96"
      width={w}
      height={h}
      aria-hidden
      className="mem-empty-svg"
    >
      <defs>
        <linearGradient id="memDiaryCover" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="var(--tone-green, #34d399)" stopOpacity="0.95" />
          <stop offset="100%" stopColor="var(--tone, #10b981)" stopOpacity="0.82" />
        </linearGradient>
      </defs>
      {/* notebook body */}
      <rect x="22" y="18" width="48" height="60" rx="6" fill="url(#memDiaryCover)" />
      <rect x="28" y="24" width="36" height="48" rx="3" fill="rgba(255,255,255,0.92)" />
      <path
        d="M34 36h24M34 44h24M34 52h16"
        stroke="rgba(15,23,42,0.18)"
        strokeWidth="2"
        strokeLinecap="round"
      />
      {/* spine */}
      <rect x="22" y="18" width="8" height="60" rx="3" fill="rgba(0,0,0,0.12)" />
      {/* pencil */}
      <g transform="rotate(-38 62 58)">
        <rect x="54" y="40" width="8" height="36" rx="2" fill="#fbbf24" />
        <path d="M54 76h8l-4 8z" fill="#f59e0b" />
        <rect x="54" y="36" width="8" height="6" rx="1" fill="#e2e8f0" />
        <path d="M56 36h4v-4a2 2 0 0 0-4 0z" fill="#94a3b8" />
      </g>
    </svg>
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

export default function MemoryPanel({ onClose }: Props) {
  const { t, locale } = useI18n();
  const flowUid = useId().replace(/:/g, "");
  const flowStrokeId = `memFlowStroke-${flowUid}`;
  const flowSoftId = `memFlowSoft-${flowUid}`;
  const [view, setView] = useState<MemoryView>("diary");
  const [, setMemoryDir] = useState("");
  const [workspaceDir, setWorkspaceDir] = useState("");
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("workspace");
  const [filterAgentId, setFilterAgentId] = useState(ALL_AGENTS);

  const [calYear, setCalYear] = useState(() => new Date().getFullYear());
  const [calMonth, setCalMonth] = useState(() => new Date().getMonth() + 1);
  const [dailyDate, setDailyDate] = useState(todayLocal());
  const [dailyDates, setDailyDates] = useState<string[]>([]);
  const [allDiaryDates, setAllDiaryDates] = useState<Set<string>>(() => new Set());
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
  const [error, setError] = useState<string | null>(null);
  const [saveMsg, setSaveMsg] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  const dreamingEnabled = dreamStatus?.enabled ?? false;

  const diaryDirty = dailyDraft !== dailySaved;
  const memoryDirty = memoryDraft !== memorySaved;
  const archiveDirty = archiveDraft !== archiveSaved;
  const dirty =
    view === "diary" ? diaryDirty : view === "longterm" ? (showArchives ? archiveDirty : memoryDirty) : false;

  const diaryCount = useMemo(() => {
    if (filterAgentId === ALL_AGENTS) return allDiaryDates.size;
    return dailyDates.length;
  }, [filterAgentId, allDiaryDates, dailyDates]);

  const markedDates = useMemo(() => {
    if (filterAgentId === ALL_AGENTS) return allDiaryDates;
    return new Set(dailyDates);
  }, [filterAgentId, allDiaryDates, dailyDates]);

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

  const enableDreaming = async () => {
    setError(null);
    setSaveMsg(null);
    try {
      const st = await invoke<DreamingStatus>("set_dreaming_enabled_cmd", { enabled: true });
      setDreamStatus(st);
      setSaveMsg(t("memory.dream.enabledHint"));
    } catch (e) {
      setError(String(e));
    }
  };

  const disableDreaming = async () => {
    setError(null);
    try {
      const st = await invoke<DreamingStatus>("set_dreaming_enabled_cmd", { enabled: false });
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
    const sets = await Promise.all(
      list.map(async (a) => {
        try {
          return await invoke<string[]>("list_daily_memory", { agentId: a.id });
        } catch {
          return [] as string[];
        }
      }),
    );
    const union = new Set<string>();
    for (const dates of sets) for (const d of dates) union.add(d);
    setAllDiaryDates(union);
  }, []);

  const loadDaily = useCallback(async (agentId: string, date: string) => {
    const dates = await invoke<string[]>("list_daily_memory", { agentId });
    setDailyDates(dates);
    const content = await invoke<string>("read_daily_memory", { date, agentId });
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
      setActiveAgentId(cfg.active_agent_id);
      setAgents(cfg.agents);
      if (filterAgentId !== ALL_AGENTS && !cfg.agents.some((a) => a.id === filterAgentId)) {
        setFilterAgentId(ALL_AGENTS);
      }
      const date = dailyDate || todayLocal();
      const agentForDaily =
        filterAgentId !== ALL_AGENTS && cfg.agents.some((a) => a.id === filterAgentId)
          ? filterAgentId
          : cfg.active_agent_id;
      await loadDaily(agentForDaily, date);
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
      await applyConfig(cfg);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, [applyConfig, refreshDreamStatus]);

  useEffect(() => {
    void bootstrap();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount once
  }, []);

  useAgentsChanged(() => {
    if (dirty) {
      void (async () => {
        try {
          const cfg = await invoke<{
            active_agent_id: string;
            agents: AgentInfo[];
          }>("get_config");
          setAgents(cfg.agents);
        } catch {
          // ignore
        }
      })();
      return;
    }
    void bootstrap();
  });

  useEffect(() => {
    if (view !== "dream") return;
    void refreshDreamStatus();
  }, [view, refreshDreamStatus]);

  const confirmIfDirty = () => {
    if (!dirty) return true;
    return window.confirm(t("memory.unsavedConfirm"));
  };

  const switchView = (next: MemoryView) => {
    if (next === view) return;
    if (!confirmIfDirty()) return;
    setView(next);
    setSaveMsg(null);
    setError(null);
    setShowArchives(false);
    if (next === "longterm" && filterAgentId === ALL_AGENTS && activeAgentId) {
      setFilterAgentId(activeAgentId);
      void (async () => {
        setLoading(true);
        try {
          const cfg = await invoke<{
            memory_dir: string;
            workspace_dir: string;
            active_agent_id: string;
            agents: AgentInfo[];
          }>("set_active_agent", { agentId: activeAgentId });
          setMemoryDir(cfg.memory_dir);
          setWorkspaceDir(cfg.workspace_dir);
          setActiveAgentId(cfg.active_agent_id);
          setAgents(cfg.agents);
          await loadMemoryMd(cfg.workspace_dir);
          await loadArchive(cfg.workspace_dir, archiveId);
        } catch (e) {
          setError(String(e));
        } finally {
          setLoading(false);
        }
      })();
    }
  };

  const switchFilterAgent = async (agentId: string) => {
    if (agentId === filterAgentId) return;
    if (!confirmIfDirty()) return;
    setFilterAgentId(agentId);
    setSaveMsg(null);
    setError(null);
    setLoading(true);
    try {
      if (agentId === ALL_AGENTS) {
        await loadDaily(activeAgentId, dailyDate);
        await loadMemoryMd(workspaceDir);
        await loadArchive(workspaceDir, archiveId);
      } else {
        const cfg = await invoke<{
          memory_dir: string;
          workspace_dir: string;
          active_agent_id: string;
          agents: AgentInfo[];
        }>("set_active_agent", { agentId });
        setMemoryDir(cfg.memory_dir);
        setWorkspaceDir(cfg.workspace_dir);
        setActiveAgentId(cfg.active_agent_id);
        setAgents(cfg.agents);
        await loadDaily(agentId, dailyDate);
        await loadMemoryMd(cfg.workspace_dir);
        await loadArchive(cfg.workspace_dir, archiveId);
        await refreshAllDiaryMarks(cfg.agents);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const switchDailyDate = async (date: string) => {
    if (date === dailyDate) return;
    if (diaryDirty && !window.confirm(t("memory.unsavedConfirm"))) return;
    setDailyDate(date);
    setSaveMsg(null);
    const agentId = filterAgentId === ALL_AGENTS ? activeAgentId : filterAgentId;
    try {
      await loadDaily(agentId, date);
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
    const agentId = filterAgentId === ALL_AGENTS ? activeAgentId : filterAgentId;
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

  const renderAgentList = (opts?: { hideAll?: boolean }) => (
    <section className="mem-card mem-agents-card" aria-label={t("memory.agents")}>
      <div className="mem-card-title">{t("memory.expertCategories")}</div>
      <div className="mem-agent-rows">
        {!opts?.hideAll && (
          <button
            type="button"
            className={`mem-agent-row ${filterAgentId === ALL_AGENTS ? "active" : ""}`}
            onClick={() => void switchFilterAgent(ALL_AGENTS)}
          >
            <span className="mem-agent-avatar" aria-hidden>
              ✦
            </span>
            <span className="mem-agent-row-name">{t("memory.allExperts")}</span>
          </button>
        )}
        {agents.map((a) => (
          <button
            key={a.id}
            type="button"
            className={`mem-agent-row ${filterAgentId === a.id ? "active" : ""}`}
            onClick={() => void switchFilterAgent(a.id)}
            title={a.path}
          >
            <span className="mem-agent-avatar" aria-hidden>
              {(a.name.trim()[0] || "A").toUpperCase()}
            </span>
            <span className="mem-agent-row-name">{a.name}</span>
            {a.is_default && (
              <span className="mem-agent-badge">{t("memory.defaultAgent")}</span>
            )}
          </button>
        ))}
      </div>
    </section>
  );

  return (
    <aside className="side-panel memory-panel">
      <div className="mem-top">
        <div className="mem-top-left">
          <h2 className="mem-top-title">
            {t("page.memory.title")}
            <span className="mem-beta">Beta</span>
          </h2>

          <nav className="mem-view-nav" aria-label={t("memory.views")}>
            <span className="mem-view-flow" aria-hidden>
              <span className="mem-view-flow-wash" />
              <svg className="mem-view-flow-svg" viewBox="0 0 320 40" preserveAspectRatio="none">
                <defs>
                  <linearGradient id={flowStrokeId} x1="0%" y1="0%" x2="100%" y2="0%">
                    <stop offset="0%" stopColor="#34d399" stopOpacity="0" />
                    <stop offset="12%" stopColor="#34d399" stopOpacity="0.9" />
                    <stop offset="48%" stopColor="#8b9cf7" stopOpacity="0.75" />
                    <stop offset="88%" stopColor="#34d399" stopOpacity="0.9" />
                    <stop offset="100%" stopColor="#34d399" stopOpacity="0" />
                  </linearGradient>
                  <linearGradient id={flowSoftId} x1="0%" y1="0%" x2="100%" y2="0%">
                    <stop offset="0%" stopColor="#6ee7b7" stopOpacity="0" />
                    <stop offset="20%" stopColor="#6ee7b7" stopOpacity="0.4" />
                    <stop offset="50%" stopColor="#a5b4fc" stopOpacity="0.32" />
                    <stop offset="80%" stopColor="#6ee7b7" stopOpacity="0.4" />
                    <stop offset="100%" stopColor="#6ee7b7" stopOpacity="0" />
                  </linearGradient>
                </defs>
                {[
                  { y: 10, dip: 9, w: 0.55 },
                  { y: 12.2, dip: 10.2, w: 0.7 },
                  { y: 14.4, dip: 11, w: 0.85 },
                  { y: 16.6, dip: 11.6, w: 1 },
                  { y: 18.8, dip: 11, w: 0.85 },
                  { y: 21, dip: 10.2, w: 0.7 },
                  { y: 23.2, dip: 9, w: 0.55 },
                  { y: 25.2, dip: 8.2, w: 0.45 },
                  { y: 27, dip: 7.4, w: 0.4 },
                ].map((line, i) => (
                  <path
                    key={i}
                    className={`mem-view-flow-path ${i % 2 === 0 ? "is-core" : "is-soft"}`}
                    stroke={`url(#${i % 2 === 0 ? flowStrokeId : flowSoftId})`}
                    strokeWidth={line.w}
                    d={`M 8 ${line.y} C 78 ${line.y}, 118 ${line.y + line.dip}, 160 ${line.y + line.dip} S 242 ${line.y}, 312 ${line.y}`}
                  />
                ))}
              </svg>
              <span className="mem-view-flow-ember mem-view-flow-ember--src" />
              <span className="mem-view-flow-ember mem-view-flow-ember--dst" />
              <span className="mem-view-flow-pulse" />
              <span className="mem-view-flow-pulse" />
              <span className="mem-view-flow-pulse" />
              <span className="mem-view-flow-pulse" />
            </span>
            {(
              [
                { id: "diary" as const, label: t("memory.view.diary"), Icon: IconBook },
                { id: "dream" as const, label: t("memory.view.dream"), Icon: IconMoon },
                { id: "longterm" as const, label: t("memory.view.longterm"), Icon: IconList },
              ] as const
            ).map((item) => (
              <button
                key={item.id}
                type="button"
                className={`mem-view-tab mem-view-tab--${item.id} ${view === item.id ? "active" : ""}`}
                onClick={() => switchView(item.id)}
              >
                <item.Icon />
                <span>{item.label}</span>
              </button>
            ))}
          </nav>
        </div>

        <div className="mem-top-stats">
          {view === "diary" && (
            <>
              <div className="mem-stat" title={t("memory.diaryTotal")}>
                <span className="mem-stat-icon" aria-hidden>
                  <IconBook width={15} height={15} />
                </span>
                <span className="mem-stat-body">
                  <strong>{diaryCount}</strong>
                  <span className="mem-stat-label">{t("memory.diaryTotal")}</span>
                </span>
              </div>
              <label className="mem-enhance" data-tip={t("memory.enhanceDiaryTip")}>
                <span className="mem-enhance-label">{t("memory.enhanceDiary")}</span>
                <button
                  type="button"
                  role="switch"
                  className="prefs-switch"
                  aria-checked={dreamingEnabled}
                  aria-label={t("memory.enhanceDiary")}
                  onClick={() => {
                    void (dreamingEnabled ? disableDreaming() : enableDreaming());
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
      {loading && <div className="mem-loading muted">{t("memory.loading")}</div>}

      <AnimatedSwitch switchKey={view} className="anim-switch--fill">
      {view === "diary" && (
        <div className="mem-split">
          <aside className="mem-sidebar">
            <section className="mem-card mem-calendar-card">
              <div className="mem-cal-header">
                <button type="button" className="ws-tool-btn" onClick={() => shiftMonth(-1)} aria-label={t("memory.prevMonth")}>
                  ‹
                </button>
                <div className="mem-cal-title">
                  {locale === "zh"
                    ? `${calYear}年${calMonth}月`
                    : new Date(calYear, calMonth - 1).toLocaleDateString("en-US", {
                        month: "long",
                        year: "numeric",
                      })}
                </div>
                <button type="button" className="ws-tool-btn" onClick={() => shiftMonth(1)} aria-label={t("memory.nextMonth")}>
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
            {renderAgentList()}
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

            {filterAgentId === ALL_AGENTS && !diaryEmpty ? (
              <div className="mem-empty mem-empty-pick">
                {onClose && (
                  <button
                    type="button"
                    className="mem-empty-chat"
                    onClick={onClose}
                    title={t("memory.back")}
                    aria-label={t("memory.back")}
                  >
                    <IconWsBackChat width={28} height={28} />
                  </button>
                )}
                <p className="mem-empty-title">{t("memory.pickExpertForDiary")}</p>
                <p className="mem-empty-sub">{t("memory.pickExpertHint")}</p>
              </div>
            ) : diaryEmpty ? (
              <div className="mem-empty mem-empty-diary">
                {onClose ? (
                  <button
                    type="button"
                    className="mem-empty-chat"
                    onClick={onClose}
                    title={t("memory.goChat")}
                    aria-label={t("memory.goChat")}
                  >
                    <IconWsBackChat width={28} height={28} />
                  </button>
                ) : (
                  <div className="mem-empty-art" aria-hidden>
                    <IconDiaryEmpty width={96} height={96} />
                  </div>
                )}
                <p className="mem-empty-title">{t("memory.diaryEmptyTitle")}</p>
                <p className="mem-empty-sub">{t("memory.diaryEmptySub")}</p>
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
                aria-label={`mermaid/${dailyDate}.md`}
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
                  <span className="muted">{t("memory.dream.runningSub")}</span>
                  {dreamStatus?.last_error && (
                    <span className="mem-dream-error">{dreamStatus.last_error}</span>
                  )}
                </div>
                <div className="mem-dream-banner-stats">
                  <span>
                    <strong>{dreamStatus?.total_points ?? 0}</strong> {t("memory.dream.points")}
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
                      dreamRunning ? t("memory.dream.runningShort") : t("memory.dream.runNow")
                    }
                  >
                    {dreamRunning ? t("memory.dream.runningShort") : t("memory.dream.runNow")}
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
                {(dreamStatus?.agents?.length ? dreamStatus.agents : agents.map((a) => ({
                  agent_id: a.id,
                  agent_name: a.name,
                  points: 0,
                  new_memories: 0,
                  pending_diaries: 0,
                  last_run_at: null,
                  last_error: null,
                }))).map((a) => (
                  <article key={a.agent_id} className="mem-card mem-dream-agent-card">
                    <div className="mem-dream-agent-head">
                      <span className="mem-agent-avatar lg" aria-hidden>
                        {(a.agent_name.trim()[0] || "A").toUpperCase()}
                      </span>
                      <div>
                        <div className="mem-dream-agent-name">{a.agent_name}</div>
                        <div className="muted mem-dream-agent-desc">
                          {a.pending_diaries > 0
                            ? t("memory.dream.agentPending", {
                                count: String(a.pending_diaries),
                              })
                            : t("memory.dream.agentDesc")}
                        </div>
                        {a.last_error && (
                          <div className="mem-dream-error">{a.last_error}</div>
                        )}
                      </div>
                    </div>
                    <div className="mem-dream-agent-foot">
                      <span>
                        {t("memory.dream.points")} <strong>{a.points}</strong>
                      </span>
                      <span>
                        {t("memory.dream.newMemories")} <strong>{a.new_memories}</strong>
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
        <div className="mem-split">
          <aside className="mem-sidebar">{renderAgentList({ hideAll: true })}</aside>
          <section className="mem-card mem-main">
            <div className="mem-main-header">
              <h3>{showArchives ? t("memory.archives") : t("memory.view.longterm")}</h3>
              <div className="memory-editor-actions">
                <button
                  type="button"
                  className={`mem-glass-btn ${showArchives ? "is-active" : ""}`}
                  onClick={() => {
                    setShowArchives((v) => !v);
                    setSaveMsg(null);
                  }}
                  title={showArchives ? t("memory.backToMemory") : t("memory.moreArchives")}
                  aria-label={showArchives ? t("memory.backToMemory") : t("memory.moreArchives")}
                >
                  {showArchives ? <IconArrowLeft /> : <IconFiles />}
                  <span>{showArchives ? t("memory.backToMemory") : t("memory.moreArchives")}</span>
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
                <div className="mem-empty-orb" aria-hidden />
                <div className="mem-empty-icon" aria-hidden>
                  <span className="mem-empty-lens" />
                  <span className="mem-empty-glyph">
                    <IconList width={26} height={26} />
                  </span>
                </div>
                <p className="mem-empty-title">{t("memory.pickExpertForMemory")}</p>
                <p className="mem-empty-sub">{t("memory.pickExpertHint")}</p>
              </div>
            ) : showArchives ? (
              <>
                <div className="memory-file-tabs" role="tablist">
                  {ARCHIVE_FILES.map((f) => (
                    <button
                      key={f.id}
                      type="button"
                      role="tab"
                      className={`memory-file-tab ${archiveId === f.id ? "active" : ""}`}
                      onClick={() => {
                        if (archiveDirty && !window.confirm(t("memory.unsavedConfirm"))) return;
                        setArchiveId(f.id);
                        void loadArchive(workspaceDir, f.id);
                      }}
                    >
                      {f.filename}
                    </button>
                  ))}
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
      </AnimatedSwitch>
    </aside>
  );
}
