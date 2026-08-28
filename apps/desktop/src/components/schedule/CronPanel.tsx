/** 定时任务面板：任务列表、运行记录与创建抽屉。 */
import { useCallback, useEffect, useMemo, useRef, useState, type SVGProps } from "react";
import { createPortal } from "react-dom";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";
import { invoke } from "@tauri-apps/api/core";
import {
  BookOpen,
  BrainCircuit,
  CalendarClock,
  CalendarPlus,
  CheckCircle2,
  ChevronRight,
  CircleAlert,
  Clock3,
  Eye,
  EyeOff,
  Film,
  GraduationCap,
  Hand,
  Heart,
  History,
  Languages,
  LayoutTemplate,
  ListTodo,
  ListTree,
  LoaderCircle,
  MessageSquareText,
  Newspaper,
  Phone,
  Server,
  Stethoscope,
  X,
  type LucideIcon,
} from "lucide-react";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { useConfirm } from "../../hooks/ui/DialogContext";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { formatScheduleLabel } from "../../lib/cron/cronSchedule";
import { mapHistoryMessages } from "../../lib/chat/mapHistoryMessages";
import type { ChatHistoryDto, ChatMessage } from "../../types";
import MotionSwitch from "../ui/MotionSwitch";
import ExpandableSearch from "../ui/ExpandableSearch";
import {
  CreateCronDialog,
  type CronPrefill,
  type ProviderOpt,
} from "./CreateCronDialog";
import {
  CronTaskDetailDrawer,
  CronRunDetailDrawer,
  type CronJobDto,
  type CronRunDto,
} from "./CronRunDetailDrawer";
import { GlassDatePicker } from "./GlassDatePicker";
import { SelectMenu } from "../ui";
import { EmptyIllustration } from "../../illustrations";

export type { CronJobDto } from "./CronRunDetailDrawer";

type CronTemplate = {
  id: string;
  icon: LucideIcon;
  titleKey: MessageKey;
  descKey: MessageKey;
  taskKey: MessageKey;
  schedule: string;
};

const CRON_TEMPLATES: CronTemplate[] = [
  {
    id: "news",
    icon: Newspaper,
    titleKey: "cron.tpl.news.title",
    descKey: "cron.tpl.news.desc",
    taskKey: "cron.tpl.news.task",
    schedule: "0 8 * * *",
  },
  {
    id: "words",
    icon: Languages,
    titleKey: "cron.tpl.words.title",
    descKey: "cron.tpl.words.desc",
    taskKey: "cron.tpl.words.task",
    schedule: "0 9 * * *",
  },
  {
    id: "story",
    icon: BookOpen,
    titleKey: "cron.tpl.story.title",
    descKey: "cron.tpl.story.desc",
    taskKey: "cron.tpl.story.task",
    schedule: "0 20 * * *",
  },
  {
    id: "weekly",
    icon: ListTodo,
    titleKey: "cron.tpl.weekly.title",
    descKey: "cron.tpl.weekly.desc",
    taskKey: "cron.tpl.weekly.task",
    schedule: "0 17 * * 5",
  },
  {
    id: "movie",
    icon: Film,
    titleKey: "cron.tpl.movie.title",
    descKey: "cron.tpl.movie.desc",
    taskKey: "cron.tpl.movie.task",
    schedule: "0 12 * * *",
  },
  {
    id: "history",
    icon: GraduationCap,
    titleKey: "cron.tpl.history.title",
    descKey: "cron.tpl.history.desc",
    taskKey: "cron.tpl.history.task",
    schedule: "0 8 * * *",
  },
  {
    id: "family",
    icon: Heart,
    titleKey: "cron.tpl.family.title",
    descKey: "cron.tpl.family.desc",
    taskKey: "cron.tpl.family.task",
    schedule: "0 10 * * 0",
  },
  {
    id: "health",
    icon: Stethoscope,
    titleKey: "cron.tpl.health.title",
    descKey: "cron.tpl.health.desc",
    taskKey: "cron.tpl.health.task",
    schedule: "0 10 * * *",
  },
  {
    id: "meeting",
    icon: Phone,
    titleKey: "cron.tpl.meeting.title",
    descKey: "cron.tpl.meeting.desc",
    taskKey: "cron.tpl.meeting.task",
    schedule: "0 9 * * 1-5",
  },
  {
    id: "interview",
    icon: BrainCircuit,
    titleKey: "cron.tpl.interview.title",
    descKey: "cron.tpl.interview.desc",
    taskKey: "cron.tpl.interview.task",
    schedule: "every:2h;wd=1,2,3,4,5",
  },
];

/** 顶栏：任务列表 / 运行历史 */
type TabId = "jobs" | "history";
/** 任务列表布局 */
type JobsView = "gallery" | "list" | "detail";

/** Cron 面板入参 */
type Props = {
  active: boolean;
  /** 可选供应商（创建任务时选模型） */
  providers: ProviderOpt[];
  activeProviderId: string | null;
  tone?: string;
};

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 上次运行时间本地化展示 */
function formatLastRun(iso: string | null, locale: "zh" | "en"): string {
  if (!iso) return "—";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(locale === "zh" ? "zh-CN" : "en-US", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** 仅时分的本地化时间 */
function formatTime(iso: string, locale: "zh" | "en"): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleTimeString(locale === "zh" ? "zh-CN" : "en-US", {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** ISO 时间戳 → `YYYY-MM-DD` 分组键 */
function dayKey(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso.slice(0, 10);
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

/** 日期键 →「今天/昨天/月日」标签 */
function formatDayLabel(key: string, locale: "zh" | "en"): string {
  const [y, m, d] = key.split("-").map(Number);
  const date = new Date(y, m - 1, d);
  if (Number.isNaN(date.getTime())) return key;
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const target = new Date(date);
  target.setHours(0, 0, 0, 0);
  const diff = (today.getTime() - target.getTime()) / 86400000;
  if (diff === 0) return locale === "zh" ? "今天" : "Today";
  if (diff === 1) return locale === "zh" ? "昨天" : "Yesterday";
  return date.toLocaleDateString(locale === "zh" ? "zh-CN" : "en-US", {
    month: "long",
    day: "numeric",
    weekday: "short",
  });
}

/** 运行状态归一化为展示种类 */
function runStatusKind(status: string): "success" | "failure" | "running" | "other" {
  const s = status.toLowerCase();
  if (s === "success" || s === "ok" || s === "completed") return "success";
  if (s === "failure" || s === "failed" || s === "error") return "failure";
  if (s === "running" || s === "in_progress" || s === "pending") return "running";
  return "other";
}

/** 运行状态对应图标 */
function RunStatusIcon({ status }: { status: string }) {
  const kind = runStatusKind(status);
  if (kind === "success") {
    return <CheckCircle2 size={12} strokeWidth={2.4} aria-hidden />;
  }
  if (kind === "failure") {
    return <CircleAlert size={12} strokeWidth={2.4} aria-hidden />;
  }
  if (kind === "running") {
    return <LoaderCircle size={12} strokeWidth={2.4} className="is-spin" aria-hidden />;
  }
  return <Clock3 size={12} strokeWidth={2.4} aria-hidden />;
}

/** 拉取定时任务运行记录（兼容两种 invoke 参数形态） */
async function fetchCronRuns(filters: {
  jobId: string;
  agentId: string;
  dateFrom: string;
  dateTo: string;
}): Promise<CronRunDto[]> {
  const payload = {
    job_id: filters.jobId || null,
    agent_id: filters.agentId || null,
    date_from: filters.dateFrom || null,
    date_to: filters.dateTo || null,
    limit: 100,
  };
  try {
    return await invoke<CronRunDto[]>("list_cron_runs", { args: payload });
  } catch {
    return await invoke<CronRunDto[]>("list_cron_runs", payload);
  }
}

/** 更多操作（三点） */
function IconMore(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden {...props}>
      <circle cx="5" cy="12" r="1.8" />
      <circle cx="12" cy="12" r="1.8" />
      <circle cx="19" cy="12" r="1.8" />
    </svg>
  );
}

/** 立即运行 */
function IconPlay(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <polygon points="6 4 20 12 6 20 6 4" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** 编辑任务 */
function IconEdit(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M12 20h9" />
      <path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" />
    </svg>
  );
}

/** 运行历史 */
function IconHistory(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" />
      <path d="M14 2v6h6" />
      <circle cx="12" cy="15" r="3" />
      <path d="M12 14v1.5l1 1" />
    </svg>
  );
}

/** 定时任务卡片主图标 */
function IconCronGlyph(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M9 3.2 7.2 5.6" />
      <path d="M15 3.2 16.8 5.6" />
      <circle cx="12" cy="13" r="8" />
      <path d="m9.2 13.1 1.9 1.9 3.8-4" />
    </svg>
  );
}

/** 删除 */
function IconTrash(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M3 6h18" />
      <path d="M8 6V4h8v2" />
      <path d="M19 6l-1 14H6L5 6" />
      <path d="M10 11v6M14 11v6" />
    </svg>
  );
}

/** 画廊视图 */
function IconViewGallery(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <rect x="3" y="3" width="7" height="7" rx="1.5" />
      <rect x="14" y="3" width="7" height="7" rx="1.5" />
      <rect x="3" y="14" width="7" height="7" rx="1.5" />
      <rect x="14" y="14" width="7" height="7" rx="1.5" />
    </svg>
  );
}

/** 列表视图 */
function IconViewList(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M8 6h13M8 12h13M8 18h13" />
      <circle cx="4" cy="6" r="1" fill="currentColor" stroke="none" />
      <circle cx="4" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="4" cy="18" r="1" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** 详情分栏视图 */
function IconViewDetail(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <rect x="3" y="4" width="8" height="16" rx="1.5" />
      <rect x="13" y="4" width="8" height="16" rx="1.5" />
    </svg>
  );
}

/** 调度时间元信息 */
function IconSchedule(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}

/** 上次运行元信息 */
function IconLastRun(props: SVGProps<SVGSVGElement>) {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden {...props}>
      <path d="M3 12a9 9 0 1 0 3-6.7" />
      <path d="M3 4v5h5" />
      <path d="M12 7v5l3 2" />
    </svg>
  );
}

export default function CronPanel({
  active,
  providers,
  activeProviderId,
  tone,
}: Props) {
  const { t, locale } = useI18n();
  const confirm = useConfirm();
  const [jobs, setJobs] = useState<CronJobDto[]>([]);
  const agentId = "default";
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);
  const [editingJob, setEditingJob] = useState<CronJobDto | null>(null);
  const [prefill, setPrefill] = useState<CronPrefill | null>(null);
  const [menuJobId, setMenuJobId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const [activeTab, setActiveTab] = useState<TabId>("jobs");
  const [jobsView, setJobsView] = useState<JobsView>("gallery");
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [detailRuns, setDetailRuns] = useState<CronRunDto[]>([]);
  const [detailRunsLoading, setDetailRunsLoading] = useState(false);
  const [drawerJobId, setDrawerJobId] = useState<string | null>(null);
  const [drawerJobRuns, setDrawerJobRuns] = useState<CronRunDto[]>([]);
  const [drawerJobRunsLoading, setDrawerJobRunsLoading] = useState(false);

  const [filterJobId, setFilterJobId] = useState("");
  const [filterDateFrom, setFilterDateFrom] = useState("");
  const [filterDateTo, setFilterDateTo] = useState("");
  const [historyRuns, setHistoryRuns] = useState<CronRunDto[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [drawerRun, setDrawerRun] = useState<CronRunDto | null>(null);
  const [drawerMessages, setDrawerMessages] = useState<ChatMessage[]>([]);
  const [drawerTraceLoading, setDrawerTraceLoading] = useState(false);

  const pageRef = useRef<HTMLDivElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);
  const moreBtnRef = useRef<HTMLButtonElement | null>(null);

  const menuPos = useAnchoredMenu({
    open: !!menuJobId,
    anchorRef: moreBtnRef,
    menuRef,
    sizeKey: menuJobId,
    fixedWidth: 176,
    preferAlign: "end",
    placement: "auto",
    gap: 6,
    maxHeightCap: 280,
  });

  const closeMenu = useCallback((restoreFocus = false) => {
    const btn = moreBtnRef.current;
    setMenuJobId(null);
    moreBtnRef.current = null;
    if (restoreFocus) btn?.focus();
  }, []);

  const openMenu = (jobId: string, btn: HTMLButtonElement) => {
    if (menuJobId === jobId) {
      closeMenu();
      return;
    }
    moreBtnRef.current = btn;
    setMenuJobId(jobId);
  };

  const loadJobs = useCallback(async () => {
    if (!isTauri()) {
      setJobs([]);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<CronJobDto[]>("list_cron_jobs");
      setJobs(list);
    } catch (err) {
      setError(String(err));
      setJobs([]);
    } finally {
      setLoading(false);
    }
  }, []);

  const loadHistoryRuns = useCallback(async () => {
    if (!isTauri()) {
      setHistoryRuns([]);
      return;
    }
    setHistoryLoading(true);
    try {
      const runs = await fetchCronRuns({
        jobId: filterJobId,
        agentId: "default",
        dateFrom: filterDateFrom,
        dateTo: filterDateTo,
      });
      setHistoryRuns(runs);
    } catch (err) {
      setError(String(err));
      setHistoryRuns([]);
    } finally {
      setHistoryLoading(false);
    }
  }, [filterJobId, filterDateFrom, filterDateTo]);

  const loadDetailRuns = useCallback(async (jobId: string) => {
    if (!isTauri()) {
      setDetailRuns([]);
      return;
    }
    setDetailRunsLoading(true);
    try {
      const runs = await invoke<CronRunDto[]>("list_cron_job_runs", { id: jobId });
      setDetailRuns(runs);
    } catch (err) {
      setError(String(err));
      setDetailRuns([]);
    } finally {
      setDetailRunsLoading(false);
    }
  }, []);

  const loadDrawerJobRuns = useCallback(async (jobId: string) => {
    if (!isTauri()) {
      setDrawerJobRuns([]);
      return;
    }
    setDrawerJobRunsLoading(true);
    try {
      const runs = await invoke<CronRunDto[]>("list_cron_job_runs", {
        id: jobId,
      });
      setDrawerJobRuns(runs);
    } catch (err) {
      setError(String(err));
      setDrawerJobRuns([]);
    } finally {
      setDrawerJobRunsLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!active) return;
    void loadJobs();
  }, [active, loadJobs]);

  useEffect(() => {
    if (!active || activeTab !== "history") return;
    void loadHistoryRuns();
  }, [active, activeTab, loadHistoryRuns]);

  useEffect(() => {
    if (!active || activeTab !== "history") return;
    const id = window.setInterval(() => void loadHistoryRuns(), 15000);
    return () => window.clearInterval(id);
  }, [active, activeTab, loadHistoryRuns]);

  useEffect(() => {
    if (!menuJobId) return;
    const onDoc = (e: MouseEvent) => {
      const target = e.target as Node;
      if (menuRef.current?.contains(target)) return;
      if (moreBtnRef.current?.contains(target)) return;
      closeMenu();
    };
    const onKey = (e: KeyboardEvent) => {
      const items = menuRef.current
        ? Array.from(
            menuRef.current.querySelectorAll<HTMLElement>('[role="menuitem"]'),
          )
        : [];
      if (e.key === "Escape") {
        e.preventDefault();
        closeMenu(true);
        return;
      }
      if (items.length === 0) return;
      const active = document.activeElement as HTMLElement | null;
      let idx = items.indexOf(active as HTMLElement);
      if (e.key === "ArrowDown") {
        e.preventDefault();
        idx = idx < 0 ? 0 : Math.min(idx + 1, items.length - 1);
        items[idx]?.focus();
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        idx = idx < 0 ? items.length - 1 : Math.max(idx - 1, 0);
        items[idx]?.focus();
      } else if (e.key === "Home") {
        e.preventDefault();
        items[0]?.focus();
      } else if (e.key === "End") {
        e.preventDefault();
        items[items.length - 1]?.focus();
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    const raf = requestAnimationFrame(() => {
      menuRef.current
        ?.querySelector<HTMLElement>('[role="menuitem"]')
        ?.focus();
    });
    return () => {
      cancelAnimationFrame(raf);
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [menuJobId, closeMenu]);

  const filteredJobs = useMemo(() => {
    const q = search.trim().toLowerCase();
    return jobs.filter((j) => {
      if (j.agent_id !== agentId && j.agent_id !== "workspace") return false;
      if (!q) return true;
      return (
        j.title.toLowerCase().includes(q) ||
        j.task.toLowerCase().includes(q) ||
        j.schedule.toLowerCase().includes(q) ||
        (j.model?.toLowerCase().includes(q) ?? false)
      );
    });
  }, [jobs, agentId, search]);

  const selectedDetailJob = useMemo(
    () => filteredJobs.find((j) => j.id === selectedDetailId) ?? null,
    [filteredJobs, selectedDetailId],
  );
  const drawerJob = useMemo(
    () => jobs.find((job) => job.id === drawerJobId) ?? null,
    [jobs, drawerJobId],
  );

  useEffect(() => {
    if (jobsView !== "detail") return;
    if (filteredJobs.length === 0) {
      setSelectedDetailId(null);
      setDetailRuns([]);
      return;
    }
    if (!selectedDetailId || !filteredJobs.some((j) => j.id === selectedDetailId)) {
      setSelectedDetailId(filteredJobs[0].id);
    }
  }, [jobsView, filteredJobs, selectedDetailId]);

  useEffect(() => {
    if (jobsView !== "detail" || !selectedDetailId) return;
    void loadDetailRuns(selectedDetailId);
  }, [jobsView, selectedDetailId, loadDetailRuns]);

  useEffect(() => {
    if (!drawerJobId) return;
    setDrawerJobRuns([]);
    void loadDrawerJobRuns(drawerJobId);
  }, [drawerJobId, loadDrawerJobRuns]);

  const providerName = useCallback(
    (id: string | null) => {
      if (!id) return "—";
      return providers.find((p) => p.id === id)?.name ?? id;
    },
    [providers],
  );

  const historyGrouped = useMemo(() => {
    const map = new Map<string, CronRunDto[]>();
    for (const run of historyRuns) {
      const key = dayKey(run.fired_at);
      const list = map.get(key) ?? [];
      list.push(run);
      map.set(key, list);
    }
    return [...map.entries()].sort((a, b) => b[0].localeCompare(a[0]));
  }, [historyRuns]);

  const jobFilterOptions = useMemo(
    () => [
      { value: "", label: t("cron.history.filterJob") },
      ...jobs.map((j) => ({ value: j.id, label: j.title })),
    ],
    [jobs, t],
  );

  const toggleEnabled = async (job: CronJobDto) => {
    if (!isTauri()) return;
    const next = !job.enabled;
    setJobs((prev) =>
      prev.map((j) => (j.id === job.id ? { ...j, enabled: next } : j)),
    );
    setError(null);
    try {
      const ok = await invoke<boolean>("set_cron_job_enabled", {
        id: job.id,
        enabled: next,
      });
      if (!ok) {
        setJobs((prev) =>
          prev.map((j) =>
            j.id === job.id ? { ...j, enabled: job.enabled } : j,
          ),
        );
        setError(t("cron.error"));
      }
    } catch (err) {
      setJobs((prev) =>
        prev.map((j) =>
          j.id === job.id ? { ...j, enabled: job.enabled } : j,
        ),
      );
      setError(String(err));
    }
  };

  const runNow = async (job: CronJobDto) => {
    if (!isTauri()) return;
    setBusyId(job.id);
    closeMenu();
    setError(null);
    try {
      const row = await invoke<CronRunDto>("run_cron_job_now", { id: job.id });
      if (drawerJobId === job.id) {
        setDrawerJobRuns((current) => [
          row,
          ...current.filter((run) => run.id !== row.id),
        ]);
      }
      await loadJobs();
      if (jobsView === "detail" && selectedDetailId === job.id) {
        void loadDetailRuns(job.id);
      }
      if (drawerJobId === job.id) {
        void loadDrawerJobRuns(job.id);
      }
      if (activeTab === "history") {
        void loadHistoryRuns();
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyId(null);
    }
  };

  const removeJob = async (job: CronJobDto) => {
    if (!isTauri()) return;
    const ok = await confirm({
      title: t("dialog.deleteTitle"),
      message: t("cron.removeConfirm"),
      confirmLabel: t("cron.remove"),
      variant: "danger",
    });
    if (!ok) return;
    setBusyId(job.id);
    closeMenu();
    setError(null);
    try {
      await invoke("remove_cron_job", { id: job.id });
      if (drawerRun?.job_id === job.id) {
        setDrawerRun(null);
        setDrawerMessages([]);
      }
      if (selectedDetailId === job.id) {
        setSelectedDetailId(null);
      }
      if (drawerJobId === job.id) {
        setDrawerJobId(null);
        setDrawerJobRuns([]);
      }
      await loadJobs();
      if (activeTab === "history") {
        void loadHistoryRuns();
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setBusyId(null);
    }
  };

  const deleteRun = async (run: CronRunDto) => {
    if (!isTauri()) return;
    if (runStatusKind(run.status) === "running") {
      setError(t("cron.history.deleteRunRunning"));
      return;
    }
    const ok = await confirm({
      title: t("dialog.deleteTitle"),
      message: t("cron.history.deleteRunConfirm"),
      confirmLabel: t("cron.history.deleteRun"),
      variant: "danger",
    });
    if (!ok) return;
    setError(null);
    try {
      await invoke("delete_cron_run", { id: run.id });
      if (drawerRun?.id === run.id) {
        setDrawerRun(null);
        setDrawerMessages([]);
      }
      setDetailRuns((prev) => prev.filter((r) => r.id !== run.id));
      setDrawerJobRuns((prev) => prev.filter((r) => r.id !== run.id));
      setHistoryRuns((prev) => prev.filter((r) => r.id !== run.id));
      if (jobsView === "detail" && selectedDetailId === run.job_id) {
        void loadDetailRuns(run.job_id);
      }
      if (drawerJobId === run.job_id) {
        void loadDrawerJobRuns(run.job_id);
      }
      if (activeTab === "history") {
        void loadHistoryRuns();
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const openHistory = (job: CronJobDto) => {
    closeMenu();
    setFilterJobId(job.id);
    setActiveTab("history");
  };

  const openRunDrawer = useCallback((run: CronRunDto) => {
    setDrawerMessages([]);
    setDrawerRun(run);
  }, []);

  const openJobDrawer = useCallback((job: CronJobDto) => {
    closeMenu();
    setDrawerJobRuns([]);
    setDrawerJobId(job.id);
  }, [closeMenu]);

  // 执行记录抽屉：轮询 run 状态 + session 历史（对齐 Tracing 步骤）
  useEffect(() => {
    if (!drawerRun || !isTauri()) return;
    let cancelled = false;
    let timer: number | null = null;
    const sid = drawerRun.session_id;
    const runId = drawerRun.id;

    const refresh = async () => {
      try {
        const latest = await invoke<CronRunDto | null>("get_cron_run", { id: runId });
        if (cancelled) return;
        if (latest) setDrawerRun(latest);

        const sessionId = latest?.session_id ?? sid;
        if (sessionId) {
          setDrawerTraceLoading(true);
          const hist = await invoke<ChatHistoryDto>("get_chat_history", {
            sessionId,
            limit: 200,
          });
          if (cancelled) return;
          setDrawerMessages(mapHistoryMessages(hist.messages ?? []));
          setDrawerTraceLoading(false);
        }

        const stillRunning =
          (latest?.status || "").toLowerCase() === "running";
        if (!stillRunning && timer != null) {
          window.clearInterval(timer);
          timer = null;
        }
      } catch (e) {
        console.warn("cron drawer refresh failed", e);
        if (!cancelled) setDrawerTraceLoading(false);
      }
    };

    void refresh();
    timer = window.setInterval(() => {
      void refresh();
    }, 1500);
    return () => {
      cancelled = true;
      if (timer != null) window.clearInterval(timer);
    };
  }, [drawerRun?.id]);

  const runStatusLabel = (status: string) => {
    const kind = runStatusKind(status);
    if (kind === "success") return t("cron.run.statusSuccess");
    if (kind === "failure") return t("cron.run.statusFailure");
    if (kind === "running") return t("cron.run.statusRunning");
    if (status === "manual") return t("cron.run.statusManual");
    if (status === "due") return t("cron.run.statusDue");
    return status;
  };

  const renderJobActions = (job: CronJobDto) => (
    <div className="cron-card-top-actions">
      <button
        type="button"
        role="switch"
        className="tool-toggle"
        aria-checked={job.enabled}
        aria-label={job.title}
        onClick={() => void toggleEnabled(job)}
      >
        <span className="tool-toggle-thumb" />
      </button>
      <div className={`cron-more ${menuJobId === job.id ? "is-open" : ""}`}>
        <button
          type="button"
          className="cron-more-btn"
          aria-haspopup="menu"
          aria-expanded={menuJobId === job.id}
          aria-label={t("cron.more")}
          disabled={busyId === job.id}
          onClick={(e) => openMenu(job.id, e.currentTarget)}
        >
          <IconMore />
        </button>
      </div>
    </div>
  );

  const renderGallery = () => (
    <div className="cron-gallery" role="list">
      {filteredJobs.map((job, index) => (
        <article
          key={job.id}
          role="listitem"
          className={`cron-card ${job.enabled ? "is-enabled" : "is-disabled"} ${
            drawerJobId === job.id ? "is-detail-open" : ""
          }`}
          style={{ animationDelay: `${0.04 + index * 0.05}s` }}
        >
          <header className="cron-card-top">
            <span className="cron-card-icon" aria-hidden>
              <span className="cron-card-lens" />
              <span className="cron-card-glyph">
                <IconCronGlyph />
              </span>
            </span>
            <div className="cron-card-heading">
              <h3 className="cron-card-title">{job.title}</h3>
              <span
                className={`cron-card-status ${job.enabled ? "is-on" : "is-off"}`}
              >
                {job.enabled ? t("cron.statusOn") : t("cron.statusOff")}
              </span>
            </div>
            <div className="cron-card-actions-cluster">
              {renderJobActions(job)}
              <button
                type="button"
                className="cron-card-detail-btn"
                onClick={() => openJobDrawer(job)}
                title={t("cron.view.detail")}
                aria-label={`${t("cron.view.detail")}: ${job.title}`}
              >
                <ChevronRight size={17} strokeWidth={2.4} aria-hidden />
              </button>
            </div>
          </header>
          <p className="cron-card-task">{job.task}</p>
          <div className="cron-card-meta">
            <div className="cron-card-meta-grid">
              <div className="cron-card-meta-cell">
                <span className="cron-card-meta-kicker">
                  <IconSchedule />
                  {t("cron.field.schedule")}
                </span>
                <span className="cron-card-meta-value is-tone">
                  {formatScheduleLabel(job.schedule, locale)}
                </span>
              </div>
              <div className="cron-card-meta-cell">
                <span className="cron-card-meta-kicker">
                  <IconLastRun />
                  {t("cron.lastRun")}
                </span>
                <span className="cron-card-meta-value">
                  {formatLastRun(job.last_run_at, locale)}
                </span>
              </div>
            </div>
            <div className="cron-card-meta-tags">
              {job.model ? (
                <span className="cron-card-tag is-model" title={job.model}>
                  {job.model}
                </span>
              ) : null}
            </div>
          </div>
        </article>
      ))}
    </div>
  );

  const renderList = () => (
    <div className="cron-job-list" role="list">
      {filteredJobs.map((job) => (
        <article
          key={job.id}
          role="listitem"
          className={`cron-job-list-row ${job.enabled ? "is-enabled" : "is-disabled"}`}
        >
          <span className="cron-job-list-icon" aria-hidden>
            <IconCronGlyph width={18} height={18} />
          </span>
          <div className="cron-job-list-main">
            <span className="cron-job-list-title">{job.title}</span>
            <span className="cron-job-list-meta">
              {formatScheduleLabel(job.schedule, locale)}
              {" · "}
              {t("cron.lastRun")}: {formatLastRun(job.last_run_at, locale)}
            </span>
          </div>
          <span
            className={`cron-card-status ${job.enabled ? "is-on" : "is-off"}`}
          >
            {job.enabled ? t("cron.statusOn") : t("cron.statusOff")}
          </span>
          {renderJobActions(job)}
        </article>
      ))}
    </div>
  );

  const renderDetail = () => (
    <div className="cron-job-detail">
      <div className="cron-job-detail-list" role="list">
        {filteredJobs.map((job) => (
          <button
            key={job.id}
            type="button"
            role="listitem"
            className={`cron-job-detail-item ${selectedDetailId === job.id ? "is-selected" : ""} ${job.enabled ? "" : "is-disabled"}`}
            onClick={() => setSelectedDetailId(job.id)}
          >
            <span className="cron-job-detail-item-title">{job.title}</span>
            <span className="cron-job-detail-item-meta">
              {formatScheduleLabel(job.schedule, locale)}
            </span>
          </button>
        ))}
      </div>
      <div className="cron-job-detail-panel">
        {selectedDetailJob ? (
          <>
            <header className="cron-job-detail-head">
              <div>
                <h3 className="cron-job-detail-title">{selectedDetailJob.title}</h3>
                <span
                  className={`cron-card-status ${selectedDetailJob.enabled ? "is-on" : "is-off"}`}
                >
                  {selectedDetailJob.enabled
                    ? t("cron.statusOn")
                    : t("cron.statusOff")}
                </span>
              </div>
              {renderJobActions(selectedDetailJob)}
            </header>

            <section className="cron-job-detail-section">
              <h4 className="cron-job-detail-label">
                <MessageSquareText size={13} strokeWidth={2.2} aria-hidden />
                {t("cron.detail.task")}
              </h4>
              <p className="cron-job-detail-task">{selectedDetailJob.task}</p>
            </section>

            <section className="cron-job-detail-meta-grid">
              <div className="cron-job-detail-meta-item">
                <span className="cron-job-detail-label">
                  <CalendarClock size={12} strokeWidth={2.2} aria-hidden />
                  {t("cron.field.schedule")}
                </span>
                <span>{formatScheduleLabel(selectedDetailJob.schedule, locale)}</span>
              </div>
              <div className="cron-job-detail-meta-item">
                <span className="cron-job-detail-label">
                  <Server size={12} strokeWidth={2.2} aria-hidden />
                  {t("cron.field.provider")}
                </span>
                <span>{providerName(selectedDetailJob.provider_id)}</span>
              </div>
              <div className="cron-job-detail-meta-item">
                <span className="cron-job-detail-label">
                  <BrainCircuit size={12} strokeWidth={2.2} aria-hidden />
                  {t("cron.field.model")}
                </span>
                <span>{selectedDetailJob.model ?? "—"}</span>
              </div>
              <div className="cron-job-detail-meta-item">
                <span className="cron-job-detail-label">
                  {selectedDetailJob.show_in_chat ? (
                    <Eye size={12} strokeWidth={2.2} aria-hidden />
                  ) : (
                    <EyeOff size={12} strokeWidth={2.2} aria-hidden />
                  )}
                  {t("cron.detail.showInChat")}
                </span>
                <span>{selectedDetailJob.show_in_chat ? t("cron.yes") : t("cron.no")}</span>
              </div>
              <div className="cron-job-detail-meta-item">
                <span className="cron-job-detail-label">
                  <History size={12} strokeWidth={2.2} aria-hidden />
                  {t("cron.lastRun")}
                </span>
                <span>{formatLastRun(selectedDetailJob.last_run_at, locale)}</span>
              </div>
            </section>

            <section className="cron-job-detail-section">
              <div className="cron-job-detail-section-head">
                <h4 className="cron-job-detail-label">
                  <History size={13} strokeWidth={2.2} aria-hidden />
                  {t("cron.detail.recentRuns")}
                </h4>
                <button
                  type="button"
                  className="cron-btn-primary cron-job-detail-run"
                  disabled={busyId === selectedDetailJob.id}
                  onClick={() => void runNow(selectedDetailJob)}
                >
                  <IconPlay width={14} height={14} />
                  {t("cron.runNow")}
                </button>
              </div>
              {detailRunsLoading && (
                <p className="cron-loading">{t("workspace.loading")}</p>
              )}
              {!detailRunsLoading && detailRuns.length === 0 && (
                <p className="cron-history-empty">{t("cron.history.empty")}</p>
              )}
              {!detailRunsLoading && detailRuns.length > 0 && (
                <ul className="cron-history-list">
                  {detailRuns.map((run) => (
                    <li key={run.id} className="cron-history-item">
                      <div className="cron-history-item-top">
                        <span className="cron-history-time">
                          <Clock3 size={12} strokeWidth={2.2} aria-hidden />
                          {formatLastRun(run.fired_at, locale)}
                        </span>
                        <span
                          className={`cron-status-pill is-${runStatusKind(run.status)}`}
                        >
                          <RunStatusIcon status={run.status} />
                          {runStatusLabel(run.status)}
                        </span>
                      </div>
                      {run.summary && (
                        <p className="cron-history-task">{run.summary}</p>
                      )}
                      <div className="cron-history-item-actions">
                        <button
                          type="button"
                          className="cron-timeline-log-btn"
                          onClick={() => openRunDrawer(run)}
                        >
                          <ListTree size={13} strokeWidth={2.2} aria-hidden />
                          {t("cron.history.viewLog")}
                        </button>
                        <button
                          type="button"
                          className="cron-timeline-log-btn is-danger"
                          disabled={runStatusKind(run.status) === "running"}
                          onClick={() => void deleteRun(run)}
                          title={t("cron.history.deleteRun")}
                          aria-label={t("cron.history.deleteRun")}
                        >
                          <IconTrash width={13} height={13} />
                          {t("cron.history.deleteRun")}
                        </button>
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </>
        ) : (
          <p className="cron-loading">{t("cron.empty")}</p>
        )}
      </div>
    </div>
  );

  const renderHistory = () => {
    if (historyLoading && historyRuns.length === 0) {
      return <p className="cron-loading">{t("workspace.loading")}</p>;
    }
    if (!historyLoading && historyRuns.length === 0) {
      return (
        <EmptyIllustration
          scene="cron"
          size="sm"
          className="cron-empty cron-empty--history"
          title={t("cron.history.empty")}
          hint={t("cron.history.emptyHint")}
        />
      );
    }
    return (
      <div className="cron-timeline">
        {historyGrouped.map(([day, runs]) => (
          <section key={day} className="cron-timeline-day">
            <h3 className="cron-timeline-day-label">{formatDayLabel(day, locale)}</h3>
            <div className="cron-timeline-items">
              {runs.map((run) => (
                <article key={run.id} className="cron-timeline-card">
                  <div className="cron-timeline-card-top">
                    <div className="cron-timeline-card-head">
                      <span className="cron-timeline-time">
                        <Clock3 size={13} strokeWidth={2.2} aria-hidden />
                        {formatTime(run.fired_at, locale)}
                      </span>
                      <span className="cron-timeline-title">{run.title}</span>
                    </div>
                    <span
                      className={`cron-status-pill is-${runStatusKind(run.status)}`}
                    >
                      <RunStatusIcon status={run.status} />
                      {runStatusLabel(run.status)}
                    </span>
                  </div>
                  <p className="cron-timeline-summary">
                    {run.summary || run.task}
                  </p>
                  <div className="cron-timeline-card-foot">
                    <span className="cron-timeline-meta">
                      {run.trigger ? (
                        <span className="cron-timeline-meta-chip">
                          {run.trigger === "manual" ? (
                            <Hand size={12} strokeWidth={2.2} aria-hidden />
                          ) : (
                            <CalendarClock size={12} strokeWidth={2.2} aria-hidden />
                          )}
                          {runStatusLabel(run.trigger)}
                        </span>
                      ) : null}
                    </span>
                    <span className="cron-history-item-actions">
                      <button
                        type="button"
                        className="cron-timeline-log-btn"
                        onClick={() => openRunDrawer(run)}
                      >
                        <ListTree size={13} strokeWidth={2.2} aria-hidden />
                        {t("cron.history.viewLog")}
                      </button>
                      <button
                        type="button"
                        className="cron-timeline-log-btn is-danger"
                        disabled={runStatusKind(run.status) === "running"}
                        onClick={() => void deleteRun(run)}
                        title={t("cron.history.deleteRun")}
                        aria-label={t("cron.history.deleteRun")}
                      >
                        <IconTrash width={13} height={13} />
                      </button>
                    </span>
                  </div>
                </article>
              ))}
            </div>
          </section>
        ))}
      </div>
    );
  };

  const openCreate = () => {
    setShowTemplates(false);
    setPrefill(null);
    setEditingJob(null);
    setShowCreate(true);
  };

  const openFromTemplate = (template: CronTemplate) => {
    setShowTemplates(false);
    setPrefill({
      title: t(template.titleKey),
      task: t(template.taskKey),
      schedule: template.schedule,
    });
    setEditingJob(null);
    setShowCreate(true);
  };

  const renderTemplateGrid = () => (
    <div className="cron-templates-grid">
      {CRON_TEMPLATES.map((template) => {
        const Icon = template.icon;
        return (
          <button
            key={template.id}
            type="button"
            className="cron-template-card"
            onClick={() => openFromTemplate(template)}
          >
            <span className="cron-template-icon" aria-hidden>
              <Icon size={20} strokeWidth={1.6} />
            </span>
            <span className="cron-template-text">
              <strong>{t(template.titleKey)}</strong>
              <span>{t(template.descKey)}</span>
            </span>
          </button>
        );
      })}
    </div>
  );

  return (
    <div className="cron-page" data-tone={tone ?? "teal"} ref={pageRef}>
      <section className="cron-pane">
        <div className="cron-toolbar">
          <div className="cron-toolbar-start">
            <h1 className="cron-toolbar-title">{t("page.cron.title")}</h1>
            <div className="cron-create-group">
              <button
                type="button"
                className="cron-toolbar-action cron-toolbar-action--primary"
                onClick={openCreate}
              >
                <CalendarPlus size={15} strokeWidth={2.2} aria-hidden />
                <span>{t("cron.create")}</span>
              </button>
              <button
                type="button"
                className="cron-toolbar-action cron-toolbar-action--primary cron-toolbar-action--template"
                onClick={() => {
                  setActiveTab("jobs");
                  setShowTemplates((value) => !value);
                }}
                aria-expanded={showTemplates}
              >
                <LayoutTemplate size={15} strokeWidth={2.2} aria-hidden />
                <span>{t("cron.createFromTemplate")}</span>
              </button>
            </div>
          </div>

          {activeTab === "jobs" && (
            <div className="cron-toolbar-end">
              <nav className="cron-tabs" aria-label={t("page.cron.title")}>
                <button
                  type="button"
                  className={`cron-tab ${activeTab === "jobs" ? "is-active" : ""}`}
                  onClick={() => setActiveTab("jobs")}
                >
                  <ListTodo size={15} strokeWidth={2.25} aria-hidden />
                  {t("cron.tab.jobs")}
                </button>
                <button
                  type="button"
                  className={`cron-tab ${activeTab === "history" ? "is-active" : ""}`}
                  onClick={() => setActiveTab("history")}
                >
                  <History size={15} strokeWidth={2.25} aria-hidden />
                  {t("cron.tab.history")}
                </button>
              </nav>
              <ExpandableSearch
                value={search}
                onChange={setSearch}
                placeholderKey="cron.searchPlaceholder"
              />
              <div
                className="cron-view-toggle"
                role="group"
                aria-label={t("page.cron.title")}
              >
                {(
                  [
                    { id: "gallery" as const, Icon: IconViewGallery, labelKey: "cron.view.gallery" as const },
                    { id: "list" as const, Icon: IconViewList, labelKey: "cron.view.list" as const },
                    { id: "detail" as const, Icon: IconViewDetail, labelKey: "cron.view.detail" as const },
                  ] as const
                ).map(({ id, Icon, labelKey }) => (
                  <button
                    key={id}
                    type="button"
                    className={`cron-view-btn ${jobsView === id ? "is-active" : ""}`}
                    onClick={() => setJobsView(id)}
                    title={t(labelKey)}
                    aria-label={t(labelKey)}
                    aria-pressed={jobsView === id}
                  >
                    <Icon />
                  </button>
                ))}
              </div>
            </div>
          )}

          {activeTab === "history" && (
            <div className="cron-toolbar-end cron-history-filters">
              <nav className="cron-tabs" aria-label={t("page.cron.title")}>
                <button
                  type="button"
                  className={`cron-tab ${activeTab === "jobs" ? "is-active" : ""}`}
                  onClick={() => setActiveTab("jobs")}
                >
                  <ListTodo size={15} strokeWidth={2.25} aria-hidden />
                  {t("cron.tab.jobs")}
                </button>
                <button
                  type="button"
                  className={`cron-tab ${activeTab === "history" ? "is-active" : ""}`}
                  onClick={() => setActiveTab("history")}
                >
                  <History size={15} strokeWidth={2.25} aria-hidden />
                  {t("cron.tab.history")}
                </button>
              </nav>
              <SelectMenu
                value={filterJobId}
                onChange={setFilterJobId}
                options={jobFilterOptions}
                aria-label={t("cron.history.filterJob")}
                className="cron-history-filter"
              />
              <div className="cron-history-dates" role="group" aria-label={t("cron.history.filterDate")}>
                <GlassDatePicker
                  value={filterDateFrom}
                  onChange={setFilterDateFrom}
                  aria-label={t("cron.history.filterDateFrom")}
                  placeholder={t("cron.history.filterDateFrom")}
                />
                <span className="cron-history-date-sep" aria-hidden>
                  –
                </span>
                <GlassDatePicker
                  value={filterDateTo}
                  onChange={setFilterDateTo}
                  aria-label={t("cron.history.filterDateTo")}
                  placeholder={t("cron.history.filterDateTo")}
                />
              </div>
            </div>
          )}
        </div>

        {activeTab === "jobs" && showTemplates && (
          <section className="cron-template-picker">
            <div className="cron-template-picker-header">
              <span>{t("cron.createFromTemplate")}</span>
              <button
                type="button"
                className="cron-template-picker-close"
                onClick={() => setShowTemplates(false)}
                aria-label={t("common.close")}
              >
                <X size={15} strokeWidth={2.2} aria-hidden />
              </button>
            </div>
            {renderTemplateGrid()}
          </section>
        )}

        <MotionSwitch switchKey={activeTab} className="anim-switch--fill">
        {activeTab === "jobs" && loading && filteredJobs.length === 0 && (
          <p className="cron-loading">{t("workspace.loading")}</p>
        )}

        {error && <p className="cron-error">{error}</p>}

        {activeTab === "jobs" && !loading && filteredJobs.length === 0 && !error && (
          <div className="cron-empty-with-templates">
            <EmptyIllustration
              scene="cron"
              className="cron-empty"
              title={t("cron.empty")}
              hint={t("cron.emptyHint")}
            >
              <button type="button" className="cron-btn-primary cron-empty-cta" onClick={openCreate}>
                <CalendarPlus size={15} strokeWidth={2.2} aria-hidden />
                {t("cron.create")}
              </button>
            </EmptyIllustration>
            {!showTemplates && <section className="cron-templates">
              <h3 className="cron-templates-title">{t("cron.templates.title")}</h3>
              {renderTemplateGrid()}
            </section>}
          </div>
        )}

        {activeTab === "jobs" && filteredJobs.length > 0 && (
          <>
            {jobsView === "gallery" && renderGallery()}
            {jobsView === "list" && renderList()}
            {jobsView === "detail" && renderDetail()}
          </>
        )}

        {activeTab === "history" && renderHistory()}
        </MotionSwitch>
      </section>

      <CreateCronDialog
        open={showCreate}
        editingJob={editingJob}
        prefill={prefill}
        onClose={() => {
          setShowCreate(false);
          setEditingJob(null);
          setPrefill(null);
        }}
        onCreated={() => void loadJobs()}
        providers={providers}
        activeProviderId={activeProviderId}
        toneStyle={toneStyleFromElement(pageRef.current)}
      />

      {menuJobId &&
        menuPos &&
        createPortal(
          <div
            ref={menuRef}
            className="cron-more-menu"
            role="menu"
            style={{
              top: menuPos.top,
              left: menuPos.left,
              maxHeight: menuPos.maxHeight,
            }}
          >
            {(() => {
              const job = jobs.find((j) => j.id === menuJobId);
              if (!job) return null;
              return (
                <>
                  <button
                    type="button"
                    className="cron-more-item"
                    role="menuitem"
                    onClick={() => void runNow(job)}
                  >
                    <IconPlay />
                    <span>{t("cron.runNow")}</span>
                  </button>
                  <button
                    type="button"
                    className="cron-more-item"
                    role="menuitem"
                    onClick={() => {
                      closeMenu();
                      setEditingJob(job);
                      setShowCreate(true);
                    }}
                  >
                    <IconEdit />
                    <span>{t("cron.edit")}</span>
                  </button>
                  <button
                    type="button"
                    className="cron-more-item"
                    role="menuitem"
                    onClick={() => openHistory(job)}
                  >
                    <IconHistory />
                    <span>{t("cron.viewHistory")}</span>
                  </button>
                  <button
                    type="button"
                    className="cron-more-item is-danger"
                    role="menuitem"
                    onClick={() => void removeJob(job)}
                  >
                    <IconTrash />
                    <span>{t("cron.remove")}</span>
                  </button>
                </>
              );
            })()}
          </div>,
          document.body,
        )}

      {drawerJob && (
        <CronTaskDetailDrawer
          job={drawerJob}
          runs={drawerJobRuns}
          runsLoading={drawerJobRunsLoading}
          busy={busyId === drawerJob.id}
          onClose={() => setDrawerJobId(null)}
          onEdit={() => {
            setEditingJob(drawerJob);
            setShowCreate(true);
          }}
          onToggleEnabled={() => void toggleEnabled(drawerJob)}
          onRunNow={() => void runNow(drawerJob)}
          onOpenRun={openRunDrawer}
        />
      )}

      {drawerRun && (
        <CronRunDetailDrawer
          run={drawerRun}
          messages={drawerMessages}
          traceLoading={drawerTraceLoading}
          onClose={() => setDrawerRun(null)}
          onDelete={() => void deleteRun(drawerRun)}
        />
      )}
    </div>
  );
}
