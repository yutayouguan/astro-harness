/** Skills 面板：已安装、商店搜索与安装。 */
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SVGProps,
} from "react";
import {
  BookOpen,
  Bot,
  Check,
  ChevronDown,
  CirclePlus,
  CloudDownload,
  Code2,
  Columns2,
  Download,
  Eye,
  ExternalLink,
  FileText,
  FolderOpen,
  HardDrive,
  Image as ImageIcon,
  LayoutGrid,
  Library,
  Link2,
  List,
  LoaderCircle,
  MoreHorizontal,
  Package,
  RefreshCw,
  Sparkles,
  Star,
  Terminal,
  Unlink2,
  User,
  X,
  type LucideIcon,
} from "lucide-react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { useI18n } from "../../i18n/LocaleContext";
import type { Locale, MessageKey } from "../../i18n/messages";
import {
  storeCardDescription,
  storeInstallCommand,
  storeSkillDetailUrl,
} from "../../lib/skills/skillInstallCommand";
import { resolveFileType } from "../../lib/filespace/fileTypeIcon";
import {
  createLazyLoadGate,
  decideLazyLoad,
  isStoreCacheFresh,
  LOCAL_SKILLS_TTL_MS,
  pageHasMore,
  storeCacheKey,
  type LazyLoadGate,
} from "../../lib/skills/skillsLazyLoad";
import {
  collectInstalledSkillKeys,
  inferFolderFromInstallRef,
  isStoreSkillInstalled as matchStoreSkillInstalled,
} from "../../lib/skills/skillInstalledMatch";
import {
  applyCheckResults,
  canUpdateSkillFromOrigin,
  filterUpdateRows,
  mergeUpdateRows,
  originMatchesSkill,
} from "../../lib/skills/skillUpdateRows";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { normalizeAgentId } from "../../types/agent";
import { useTransientToast } from "../../hooks/ui/useTransientToast";
import MotionSwitch from "../ui/MotionSwitch";
import ExpandableSearch from "../ui/ExpandableSearch";
import MsgStreamLoader from "../chat/MsgStreamLoader";
import { CopyMorphIcon } from "../icons/MorphIcon";
import McpIcon from "../icons/McpIcon";
import { IconRefresh } from "../icons/NavIcons";
import { useMcpSection } from "./McpSection";
import { SelectMenu } from "../ui/SelectMenu";
import EmptyIllustration from "../../illustrations/EmptyIllustration";
import {
  SkillFileViewer,
  SKILL_PREVIEW_MAX_BYTES,
} from "./SkillFileViewer";
import type {
  InstalledSkill,
  SkillBundle,
  SkillBackupEntry,
  SkillFileEntry,
  SkillOriginRecord,
  SkillUpdateCheckResult,
  SkillUpdateFilter,
  SkillUpdateItemResult,
  SkillUpdatePreview,
  SkillUpdateRow,
  StoreSkill,
  StoreSkillDetail,
  SkillStoreId,
} from "../../types";

type SkillPreviewCategory =
  | "overview"
  | "scripts"
  | "references"
  | "assets"
  | "other";

const PREVIEW_TABS: {
  id: SkillPreviewCategory;
  labelKey: MessageKey;
  Icon: LucideIcon;
}[] = [
  { id: "overview", labelKey: "skills.previewTab.overview", Icon: BookOpen },
  { id: "scripts", labelKey: "skills.previewTab.scripts", Icon: Code2 },
  { id: "references", labelKey: "skills.previewTab.references", Icon: Library },
  { id: "assets", labelKey: "skills.previewTab.assets", Icon: ImageIcon },
  { id: "other", labelKey: "skills.previewTab.other", Icon: MoreHorizontal },
];

function fileLabel(path: string): string {
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Skills 面板入参 */
export type SkillsPanelProps = {
  /** 面板是否可见（用于懒加载 / 刷新） */
  active: boolean;
  /** 跳转对话并用 Agent 安装（填入安装 Prompt） */
  onInstallWithAgent?: (prompt: string) => void;
  /** 打开时落到该 tab；消费后通知父级清空 */
  initialTab?: PluginsPrimaryTab | null;
  onInitialTabConsumed?: () => void;
  tone?: string;
};

type PluginsPrimaryTab = "skills" | "mcp";
type PluginScope = "global" | "builtin" | "project";
type PersonalSkillsTab = "installed" | "machine" | "online";
type SkillsDrawer = "updates";
type SkillInstallTarget = "global" | "project";
/** 内容布局：画廊 / 列表 / 详情 */
type SkillsView = "gallery" | "list" | "detail";
/** 已安装列表排序 */
type CallSort = "name" | "calls";
/** 本机技能链接过滤 */
type MachineLinkFilter = "all" | "linked" | "unlinked";
/** 商店列表排序 */
type StoreSort = "default" | "installs";

const UPDATE_FILTERS: {
  id: SkillUpdateFilter;
  labelKey: MessageKey;
}[] = [
  { id: "updatable", labelKey: "skills.updatesFilter.updatable" },
  { id: "with_origin", labelKey: "skills.updatesFilter.withOrigin" },
  { id: "no_origin", labelKey: "skills.updatesFilter.noOrigin" },
];

/** 按 Agent 汇总的工具/技能调用次数 */
type AgentUsageSummary = {
  agent_id: string;
  tool_total: number;
  skill_total: number;
  tools: Record<string, number>;
  skills: Record<string, number>;
};

const STORE_PAGE_SIZE = 24;
const SKILLS_VIEW_KEY = "astro.skills.viewMode";

/** 单个商店 Tab 的会话缓存快照（SWR） */
type StoreListCacheEntry = {
  results: StoreSkill[];
  page: number;
  hasMore: boolean;
  selectedDetailId: string | null;
  fetchedAt: number;
};

const SKILL_TONES = [
  "cyan",
  "blue",
  "teal",
  "indigo",
  "purple",
  "green",
  "amber",
  "orange",
] as const;

/** 从 localStorage 读取 Skills 视图模式 */
function readSkillsView(): SkillsView {
  try {
    const v = localStorage.getItem(SKILLS_VIEW_KEY);
    if (v === "gallery" || v === "list" || v === "detail") return v;
  } catch {
    // ignore
  }
  return "gallery";
}

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 按 skill id 哈希映射色调，保证同 id 颜色稳定 */
function skillTone(id: string): (typeof SKILL_TONES)[number] {
  let hash = 0;
  for (let i = 0; i < id.length; i += 1) {
    hash = (hash * 31 + id.charCodeAt(i)) >>> 0;
  }
  return SKILL_TONES[hash % SKILL_TONES.length];
}

/** 商店来源短徽章文案 */
function storeBadge(store: string): string {
  if (store === "skillhub") return "SH";
  if (store === "clawhub") return "CH";
  return "S·";
}

/** 安装量缩写（K / M） */
function formatInstalls(n?: number | null): string {
  if (n == null) return "";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** SkillHub `updated_at`（毫秒）→ 相对时间文案 */
function formatStoreUpdatedAt(
  ts: number | null | undefined,
  t: (key: MessageKey) => string,
): string | null {
  if (ts == null || !Number.isFinite(ts)) return null;
  const ms = ts > 1e12 ? ts : ts * 1000;
  const days = Math.max(0, Math.floor((Date.now() - ms) / 86_400_000));
  if (days <= 0) return t("skills.detailUpdatedToday");
  if (days < 30) {
    return t("skills.detailUpdatedDays").replace("{days}", String(days));
  }
  const months = Math.floor(days / 30);
  if (months < 12) {
    return t("skills.detailUpdatedMonths").replace("{months}", String(months));
  }
  const years = Math.floor(months / 12);
  return t("skills.detailUpdatedYears").replace("{years}", String(years));
}

/** usage-stats.json 的 skills 键为 skill_id（技能名） */
function skillCallCount(
  calls: Record<string, number>,
  skill: { name: string; id?: string },
): number {
  const candidates = [skill.name, skill.id].filter(
    (v): v is string => typeof v === "string" && v.trim().length > 0,
  );
  for (const name of candidates) {
    const direct = calls[name];
    if (typeof direct === "number") return direct;
  }
  for (const name of candidates) {
    const lower = name.toLowerCase();
    for (const [key, value] of Object.entries(calls)) {
      if (key.toLowerCase() === lower) return value;
    }
  }
  return 0;
}

/** 先按调用次数降序，次数相同再按名称排序 */
function compareByCallsThenName(
  aName: string,
  bName: string,
  aCalls: number,
  bCalls: number,
): number {
  if (bCalls !== aCalls) return bCalls - aCalls;
  return aName.toLowerCase().localeCompare(bName.toLowerCase());
}

/** 绝对路径缩写为 `~/…`（macOS / Linux home） */
function formatTildePath(path: string): string {
  if (!path) return "";
  const home = "/Users/";
  if (path.startsWith(home)) {
    const rest = path.slice(home.length);
    const slash = rest.indexOf("/");
    if (slash >= 0) return `~${rest.slice(slash)}`;
  }
  if (path.startsWith("/home/")) {
    const rest = path.slice("/home/".length);
    const slash = rest.indexOf("/");
    if (slash >= 0) return `~${rest.slice(slash)}`;
  }
  return path;
}

/** 备份目录时间戳 → 本地化日期时间 */
function formatBackupTime(entry: SkillBackupEntry, locale: Locale): string {
  const secs =
    entry.created_at ??
    (Number.isFinite(Number(entry.timestamp)) ? Number(entry.timestamp) : null);
  if (secs == null) return entry.timestamp;
  return new Date(secs * 1000).toLocaleString(
    locale === "zh" ? "zh-CN" : "en-US",
    {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
    },
  );
}

/** 加载中旋转图标 */
/** 加载中旋转图标 */
function IconLoader(props: SVGProps<SVGSVGElement>) {
  return <LoaderCircle size={18} strokeWidth={2} aria-hidden {...props} />;
}

/** 画廊视图图标 */
function IconViewGallery(props: SVGProps<SVGSVGElement>) {
  return <LayoutGrid size={15} strokeWidth={2} aria-hidden {...props} />;
}

/** 列表视图图标 */
function IconViewList(props: SVGProps<SVGSVGElement>) {
  return <List size={15} strokeWidth={2} aria-hidden {...props} />;
}

/** 详情视图图标 */
function IconViewDetail(props: SVGProps<SVGSVGElement>) {
  return <Columns2 size={15} strokeWidth={2} aria-hidden {...props} />;
}

const VIEW_OPTIONS = [
  {
    id: "gallery" as const,
    Icon: IconViewGallery,
    labelKey: "skills.view.gallery" as MessageKey,
  },
  {
    id: "list" as const,
    Icon: IconViewList,
    labelKey: "skills.view.list" as MessageKey,
  },
  {
    id: "detail" as const,
    Icon: IconViewDetail,
    labelKey: "skills.view.detail" as MessageKey,
  },
];

type SkillUpdateConfirmState =
  | {
      mode: "single";
      row: SkillUpdateRow;
      folder: string;
    }
  | {
      mode: "batch";
      targets: SkillUpdateRow[];
      dirtyCount: number;
    };

export default function SkillsPanel({
  active,
  onInstallWithAgent,
  initialTab = null,
  onInitialTabConsumed,
  tone,
}: SkillsPanelProps) {
  const { t, locale } = useI18n();
  const { showToast, toastHost } = useTransientToast();
  const [primaryTab, setPrimaryTab] = useState<PluginsPrimaryTab>("skills");
  const [scope, setScope] = useState<PluginScope>("global");
  const [personalTab, setPersonalTab] = useState<PersonalSkillsTab>("installed");
  const [drawer, setDrawer] = useState<SkillsDrawer | null>(null);
  const [installed, setInstalled] = useState<InstalledSkill[]>([]);
  const [machineSkills, setMachineSkills] = useState<InstalledSkill[]>([]);
  const [storeResults, setStoreResults] = useState<StoreSkill[]>([]);
  const [storeId, setStoreId] = useState<SkillStoreId | "all">("all");
  const [query, setQuery] = useState("");
  const [installedQuery, setInstalledQuery] = useState("");
  const [machineQuery, setMachineQuery] = useState("");
  const [mcpQuery, setMcpQuery] = useState("");
  const [loadingInstalled, setLoadingInstalled] = useState(false);
  const [loadingMachine, setLoadingMachine] = useState(false);
  const [loadingStore, setLoadingStore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [storePage, setStorePage] = useState(1);
  const [hasMore, setHasMore] = useState(true);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [installPromptSkill, setInstallPromptSkill] = useState<StoreSkill | null>(null);
  const [installTarget, setInstallTarget] = useState<SkillInstallTarget>("global");
  const [linkingId, setLinkingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const { activeAgentId: agentId } = useActiveAgent();
  const [viewMode, setViewMode] = useState<SkillsView>(() => readSkillsView());
  const pageRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!initialTab) return;
    setPrimaryTab(initialTab);
    onInitialTabConsumed?.();
  }, [initialTab, onInitialTabConsumed]);
  useEffect(() => {
    if (primaryTab !== "skills" || scope !== "global") {
      setPersonalTab("installed");
    }
    setDrawer(null);
  }, [primaryTab, scope]);
  useEffect(() => {
    if (!installPromptSkill) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !installingId) {
        setInstallPromptSkill(null);
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [installPromptSkill, installingId]);
  const mcp = useMcpSection({
    active: active && primaryTab === "mcp",
    query: mcpQuery,
    viewMode,
    hostRef: pageRef,
    scope,
  });
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [loadingPreview, setLoadingPreview] = useState<string | null>(null);
  const [preview, setPreview] = useState<SkillBundle | null>(null);
  const [previewSkillId, setPreviewSkillId] = useState<string | null>(null);
  const [previewTab, setPreviewTab] = useState<SkillPreviewCategory>("overview");
  const [previewFile, setPreviewFile] = useState<string | null>(null);
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [previewBinaryHint, setPreviewBinaryHint] = useState(false);
  const [loadingFile, setLoadingFile] = useState(false);
  const [skillCalls, setSkillCalls] = useState<Record<string, number>>({});
  const [installedSort, setInstalledSort] = useState<CallSort>("name");
  const [machineSort, setMachineSort] = useState<CallSort>("name");
  const [machineLinkFilter, setMachineLinkFilter] =
    useState<MachineLinkFilter>("all");
  const [storeSort, setStoreSort] = useState<StoreSort>("default");
  const [storeDetail, setStoreDetail] = useState<StoreSkillDetail | null>(null);
  const [loadingStoreDetail, setLoadingStoreDetail] = useState(false);
  const [origins, setOrigins] = useState<SkillOriginRecord[]>([]);
  const [loadingOrigins, setLoadingOrigins] = useState(false);
  const [updateFilter, setUpdateFilter] =
    useState<SkillUpdateFilter>("updatable");
  const [lastCheckResults, setLastCheckResults] = useState<
    SkillUpdateCheckResult[]
  >([]);
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updatingFolder, setUpdatingFolder] = useState<string | null>(null);
  const [updatingAll, setUpdatingAll] = useState(false);
  const [updateConfirm, setUpdateConfirm] = useState<SkillUpdateConfirmState | null>(
    null,
  );
  const [skillBackups, setSkillBackups] = useState<SkillBackupEntry[]>([]);
  const [loadingBackups, setLoadingBackups] = useState(false);
  const [backupsOpen, setBackupsOpen] = useState(true);

  const loadMoreLock = useRef(false);
  /** 递增以丢弃切换 Tab / 重新搜索后的过期响应 */
  const storeFetchGen = useRef(0);
  const storeCacheRef = useRef<Map<string, StoreListCacheEntry>>(new Map());
  /** 当前列表对应的缓存键（storeId + 已提交搜索词） */
  const activeStoreCacheKeyRef = useRef<string | null>(null);
  const storePageRef = useRef(1);
  const selectedDetailIdRef = useRef<string | null>(null);
  const installedMetaRef = useRef<{
    agentId: string;
    scope: PluginScope;
    fetchedAt: number;
  } | null>(null);
  const machineMetaRef = useRef<{ agentId: string; fetchedAt: number } | null>(
    null,
  );
  const originsMetaRef = useRef<{ agentId: string; fetchedAt: number } | null>(
    null,
  );
  const updateCheckMetaRef = useRef<{ agentId: string; checkedAt: number } | null>(
    null,
  );
  const updateCheckInFlightRef = useRef(false);
  const skillCallsMetaRef = useRef<{
    agentId: string;
    fetchedAt: number;
  } | null>(null);
  const installedRef = useRef<InstalledSkill[]>([]);
  const machineRef = useRef<InstalledSkill[]>([]);
  const originsRef = useRef<SkillOriginRecord[]>([]);
  const lazyGateRef = useRef<LazyLoadGate>(createLazyLoadGate());
  const sentinelRef = useRef<HTMLDivElement | null>(null);
  const onlinePaneRef = useRef<HTMLElement | null>(null);
  const galleryScrollRef = useRef<HTMLDivElement | null>(null);
  const detailListRef = useRef<HTMLDivElement | null>(null);
  const queryRef = useRef(query);
  const storeIdRef = useRef(storeId);
  const storeResultsRef = useRef<StoreSkill[]>([]);
  const loadMoreRef = useRef<() => Promise<void>>(async () => {});
  const loadingMoreRef = useRef(false);
  const hasMoreRef = useRef(true);
  queryRef.current = query;
  storeIdRef.current = storeId;
  storeResultsRef.current = storeResults;
  loadingMoreRef.current = loadingMore;
  hasMoreRef.current = hasMore;
  storePageRef.current = storePage;
  selectedDetailIdRef.current = selectedDetailId;
  installedRef.current = installed;
  machineRef.current = machineSkills;
  originsRef.current = origins;

  useEffect(() => {
    try {
      localStorage.setItem(SKILLS_VIEW_KEY, viewMode);
    } catch {
      // ignore
    }
  }, [viewMode]);

  useEffect(() => {
    setLastCheckResults([]);
    updateCheckMetaRef.current = null;
    setSkillBackups([]);
  }, [agentId]);

  const refreshInstalled = useCallback(
    async (opts?: { mode?: "hard" | "silent" }) => {
      if (!isTauri()) {
        setInstalled([]);
        installedRef.current = [];
        return;
      }
      const contextChanged =
        installedMetaRef.current?.agentId !== agentId ||
        installedMetaRef.current?.scope !== scope;
      if (contextChanged) {
        setInstalled([]);
        installedRef.current = [];
        installedMetaRef.current = null;
      }
      const silent =
        opts?.mode === "silent" && installedRef.current.length > 0 && !contextChanged;
      if (!silent) setLoadingInstalled(true);
      setError(null);
      try {
        const list = await invoke<InstalledSkill[]>("list_installed_skills", {
          agentId,
          scope,
        });
        setInstalled(list);
        installedRef.current = list;
        installedMetaRef.current = { agentId, scope, fetchedAt: Date.now() };
      } catch (err) {
        setError(String(err));
      } finally {
        if (!silent) setLoadingInstalled(false);
      }
    },
    [agentId, scope],
  );

  const refreshMachine = useCallback(
    async (opts?: { mode?: "hard" | "silent" }) => {
      if (!isTauri()) {
        setMachineSkills([]);
        machineRef.current = [];
        return;
      }
      const agentChanged = machineMetaRef.current?.agentId !== agentId;
      if (agentChanged) {
        setMachineSkills([]);
        machineRef.current = [];
        machineMetaRef.current = null;
      }
      const silent =
        opts?.mode === "silent" && machineRef.current.length > 0 && !agentChanged;
      if (!silent) setLoadingMachine(true);
      setError(null);
      try {
        const list = await invoke<InstalledSkill[]>("list_installed_skills", {
          agentId,
          scope: "machine",
        });
        setMachineSkills(list);
        machineRef.current = list;
        machineMetaRef.current = { agentId, fetchedAt: Date.now() };
      } catch (err) {
        setError(String(err));
      } finally {
        if (!silent) setLoadingMachine(false);
      }
    },
    [agentId],
  );

  const refreshOrigins = useCallback(
    async (opts?: { mode?: "hard" | "silent" }) => {
      if (!isTauri()) {
        setOrigins([]);
        originsRef.current = [];
        return;
      }
      const agentChanged = originsMetaRef.current?.agentId !== agentId;
      if (agentChanged) {
        setOrigins([]);
        originsRef.current = [];
        originsMetaRef.current = null;
      }
      const silent =
        opts?.mode === "silent" && originsRef.current.length > 0 && !agentChanged;
      if (!silent) setLoadingOrigins(true);
      setError(null);
      try {
        const list = await invoke<SkillOriginRecord[]>("list_skill_origins", {
          agentId,
        });
        setOrigins(list);
        originsRef.current = list;
        originsMetaRef.current = { agentId, fetchedAt: Date.now() };
      } catch (err) {
        setError(String(err));
      } finally {
        if (!silent) setLoadingOrigins(false);
      }
    },
    [agentId],
  );

  const refreshSkillCalls = useCallback(
    async (opts?: { force?: boolean }) => {
      if (!isTauri()) {
        setSkillCalls({});
        return;
      }
      const meta = skillCallsMetaRef.current;
      if (
        !opts?.force &&
        meta?.agentId === agentId &&
        isStoreCacheFresh(meta.fetchedAt, Date.now(), LOCAL_SKILLS_TTL_MS)
      ) {
        return;
      }
      try {
        const stats = await invoke<AgentUsageSummary>("get_agent_usage_stats", {
          agentId,
        });
        setSkillCalls(stats.skills ?? {});
        skillCallsMetaRef.current = { agentId, fetchedAt: Date.now() };
      } catch {
        setSkillCalls({});
      }
    },
    [agentId],
  );

  const loadSkillBackups = useCallback(async () => {
    if (!isTauri()) {
      setSkillBackups([]);
      return;
    }
    setLoadingBackups(true);
    try {
      const list = await invoke<SkillBackupEntry[]>("list_skill_backups", {
        agentId,
      });
      setSkillBackups(list);
    } catch {
      setSkillBackups([]);
    } finally {
      setLoadingBackups(false);
    }
  }, [agentId]);

  const checkSkillUpdates = useCallback(
    async (opts?: { force?: boolean }) => {
      if (!isTauri()) return;
      const force = opts?.force ?? false;
      const meta = updateCheckMetaRef.current;
      if (!force && meta?.agentId === agentId) return;
      if (updateCheckInFlightRef.current) return;
      updateCheckInFlightRef.current = true;
      setCheckingUpdates(true);
      setError(null);
      try {
        const checks = await invoke<SkillUpdateCheckResult[]>("check_skill_updates", {
          agentId,
        });
        setLastCheckResults(checks);
        updateCheckMetaRef.current = { agentId, checkedAt: Date.now() };
        const count = checks.filter((c) => c.status === "outdated").length;
        if (force) {
          showToast(
            count > 0
              ? t("skills.checkUpdatesDone").replace("{count}", String(count))
              : t("skills.upToDate"),
            { tone: count > 0 ? "info" : "success" },
          );
        } else if (count > 0) {
          showToast(
            t("skills.checkUpdatesDone").replace("{count}", String(count)),
            { tone: "info" },
          );
        }
        void loadSkillBackups();
      } catch (err) {
        const msg = String(err);
        setError(msg);
        showToast(msg, { error: true });
      } finally {
        updateCheckInFlightRef.current = false;
        setCheckingUpdates(false);
      }
    },
    [agentId, loadSkillBackups, showToast, t],
  );

  /** 切 Tab：新鲜则跳过；过期则静默刷新；Agent 变更/无数据则硬刷 */
  const ensureInstalled = useCallback(
    (force = false) => {
      const meta = installedMetaRef.current;
      const hasData = installedRef.current.length > 0;
      const sameAgent = meta?.agentId === agentId && meta?.scope === scope;
      if (
        !force &&
        sameAgent &&
        hasData &&
        meta &&
        isStoreCacheFresh(meta.fetchedAt, Date.now(), LOCAL_SKILLS_TTL_MS)
      ) {
        return;
      }
      if (!force && sameAgent && hasData) {
        void refreshInstalled({ mode: "silent" });
        return;
      }
      void refreshInstalled({ mode: force || !hasData ? "hard" : "silent" });
    },
    [agentId, scope, refreshInstalled],
  );

  const ensureMachine = useCallback(
    (force = false) => {
      const meta = machineMetaRef.current;
      const hasData = machineRef.current.length > 0;
      const sameAgent = meta?.agentId === agentId;
      if (
        !force &&
        sameAgent &&
        hasData &&
        meta &&
        isStoreCacheFresh(meta.fetchedAt, Date.now(), LOCAL_SKILLS_TTL_MS)
      ) {
        return;
      }
      if (!force && sameAgent && hasData) {
        void refreshMachine({ mode: "silent" });
        return;
      }
      void refreshMachine({ mode: force || !hasData ? "hard" : "silent" });
    },
    [agentId, refreshMachine],
  );

  const persistActiveStoreCache = useCallback(() => {
    const key = activeStoreCacheKeyRef.current;
    if (!key) return;
    storeCacheRef.current.set(key, {
      results: storeResultsRef.current,
      page: storePageRef.current,
      hasMore: hasMoreRef.current,
      selectedDetailId: selectedDetailIdRef.current,
      fetchedAt: storeCacheRef.current.get(key)?.fetchedAt ?? Date.now(),
    });
  }, []);

  const applyStoreCache = useCallback((entry: StoreListCacheEntry) => {
    storeResultsRef.current = entry.results;
    setStoreResults(entry.results);
    setStorePage(entry.page);
    storePageRef.current = entry.page;
    setHasMore(entry.hasMore);
    hasMoreRef.current = entry.hasMore;
    setSelectedDetailId(entry.selectedDetailId);
    selectedDetailIdRef.current = entry.selectedDetailId;
    setStoreDetail(null);
    setLoadingStore(false);
    setLoadingMore(false);
    loadMoreLock.current = false;
    lazyGateRef.current = createLazyLoadGate();
  }, []);

  const fetchStorePage = useCallback(
    async (
      page: number,
      append: boolean,
      opts?: { mode?: "hard" | "silent" },
    ) => {
      if (!isTauri()) return;
      const mode = opts?.mode ?? (append ? "hard" : "hard");
      const silent = !append && mode === "silent";
      const gen = append ? storeFetchGen.current : ++storeFetchGen.current;
      const store = storeIdRef.current;
      const q = queryRef.current.trim();
      const cacheKey = storeCacheKey(store, q);

      if (append) {
        if (loadMoreLock.current) return;
        loadMoreLock.current = true;
        setLoadingMore(true);
      } else if (silent) {
        loadMoreLock.current = false;
        setLoadingMore(false);
      } else {
        // hard：清空并展示点阵，用于首次加载 / 搜索变更 / 强制刷新
        loadMoreLock.current = false;
        setLoadingMore(false);
        storeResultsRef.current = [];
        setStoreResults([]);
        setStorePage(1);
        storePageRef.current = 1;
        setSelectedDetailId(null);
        selectedDetailIdRef.current = null;
        setStoreDetail(null);
        lazyGateRef.current = createLazyLoadGate();
        setLoadingStore(true);
        setHasMore(true);
        hasMoreRef.current = true;
      }
      activeStoreCacheKeyRef.current = cacheKey;
      setError(null);
      try {
        const list = await invoke<StoreSkill[]>("search_store_skills", {
          query: q,
          store,
          limit: STORE_PAGE_SIZE,
          page,
        });
        if (gen !== storeFetchGen.current) return;
        if (!append) {
          storeResultsRef.current = list;
          setStoreResults(list);
          const more = list.length >= STORE_PAGE_SIZE;
          setHasMore(more);
          hasMoreRef.current = more;
          setStorePage(page);
          storePageRef.current = page;
          if (
            selectedDetailIdRef.current &&
            !list.some((s) => s.id === selectedDetailIdRef.current)
          ) {
            setSelectedDetailId(null);
            selectedDetailIdRef.current = null;
            setStoreDetail(null);
          }
          storeCacheRef.current.set(cacheKey, {
            results: list,
            page,
            hasMore: more,
            selectedDetailId: selectedDetailIdRef.current,
            fetchedAt: Date.now(),
          });
        } else {
          const prev = storeResultsRef.current;
          const seen = new Set(prev.map((s) => s.id));
          const merged = [...prev];
          let newlyAdded = 0;
          for (const item of list) {
            if (!seen.has(item.id)) {
              seen.add(item.id);
              merged.push(item);
              newlyAdded += 1;
            }
          }
          storeResultsRef.current = merged;
          setStoreResults(merged);
          const more = pageHasMore(list.length, STORE_PAGE_SIZE, newlyAdded);
          setHasMore(more);
          hasMoreRef.current = more;
          setStorePage(page);
          storePageRef.current = page;
          storeCacheRef.current.set(cacheKey, {
            results: merged,
            page,
            hasMore: more,
            selectedDetailId: selectedDetailIdRef.current,
            fetchedAt:
              storeCacheRef.current.get(cacheKey)?.fetchedAt ?? Date.now(),
          });
        }
      } catch (err) {
        if (gen !== storeFetchGen.current) return;
        setError(String(err));
        if (!append && !silent) {
          setStoreResults([]);
          storeResultsRef.current = [];
        }
        if (!append && !silent) {
          setHasMore(false);
          hasMoreRef.current = false;
        }
      } finally {
        if (gen !== storeFetchGen.current) {
          if (append) loadMoreLock.current = false;
          return;
        }
        if (append) {
          setLoadingMore(false);
          loadMoreLock.current = false;
        } else if (!silent) {
          setLoadingStore(false);
        }
      }
    },
    [],
  );

  const searchStore = useCallback(async () => {
    // 搜索词变更：作废当前键并硬刷新
    persistActiveStoreCache();
    activeStoreCacheKeyRef.current = null;
    await fetchStorePage(1, false, { mode: "hard" });
  }, [fetchStorePage, persistActiveStoreCache]);

  const loadMore = useCallback(async () => {
    if (!hasMore || loadingStore || loadingMore || loadMoreLock.current) return;
    await fetchStorePage(storePage + 1, true);
  }, [fetchStorePage, hasMore, loadingMore, loadingStore, storePage]);

  loadMoreRef.current = loadMore;

  useEffect(() => {
    if (!active || primaryTab !== "skills") return;
    ensureInstalled();
    void refreshSkillCalls();
  }, [active, primaryTab, ensureInstalled, refreshSkillCalls]);

  useEffect(() => {
    if (!active || personalTab !== "machine") return;
    ensureMachine();
    void refreshSkillCalls();
  }, [active, personalTab, ensureMachine, refreshSkillCalls]);

  useEffect(() => {
    if (!active || drawer !== "updates") return;
    ensureInstalled();
    ensureMachine();
    void refreshOrigins();
    void refreshSkillCalls();
    void checkSkillUpdates();
    void loadSkillBackups();
  }, [
    active,
    drawer,
    agentId,
    ensureInstalled,
    ensureMachine,
    refreshOrigins,
    refreshSkillCalls,
    checkSkillUpdates,
    loadSkillBackups,
  ]);

  useEffect(() => {
    if (!active || personalTab !== "online") return;
    // 在线安装态依赖本机/Astro 列表：有缓存则轻量保证新鲜，不硬刷
    ensureInstalled();
    ensureMachine();
  }, [active, personalTab, ensureInstalled, ensureMachine]);

  useEffect(() => {
    if (!active || personalTab !== "online") return;

    // SWR：先写入上一 Tab 快照，再恢复缓存；过期则后台静默刷新
    persistActiveStoreCache();
    const key = storeCacheKey(storeId, queryRef.current);
    const cached = storeCacheRef.current.get(key);
    activeStoreCacheKeyRef.current = key;

    if (cached) {
      applyStoreCache(cached);
      if (!isStoreCacheFresh(cached.fetchedAt)) {
        void fetchStorePage(1, false, { mode: "silent" });
      }
      return;
    }

    void fetchStorePage(1, false, { mode: "hard" });
  }, [
    active,
    personalTab,
    storeId,
    fetchStorePage,
    persistActiveStoreCache,
    applyStoreCache,
  ]);

  useEffect(() => {
    if (!active || personalTab !== "online" || !hasMore || loadingStore) return;
    const node = sentinelRef.current;
    const root =
      viewMode === "detail"
        ? detailListRef.current
        : galleryScrollRef.current;
    if (!node || !root) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        if (!entry) return;
        const decision = decideLazyLoad(lazyGateRef.current, {
          isIntersecting: entry.isIntersecting,
          hasMore: hasMoreRef.current,
          isLoading: loadingMoreRef.current || loadMoreLock.current,
        });
        lazyGateRef.current = decision.next;
        if (decision.shouldLoad) {
          void loadMoreRef.current();
        }
      },
      { root, rootMargin: "160px", threshold: 0 },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [active, personalTab, storeId, hasMore, loadingStore, viewMode, storeResults.length]);

  const storeLoadMoreFooter = (
    <>
      {viewMode !== "detail" && (
        <div
          ref={sentinelRef}
          className="skills-scroll-sentinel"
          aria-hidden
        />
      )}
      {loadingMore && (
        <div className="skills-load-more is-loading" aria-busy="true">
          <IconLoader className="is-spin" />
          <span>{t("skills.loadingMore")}</span>
        </div>
      )}
      {!hasMore && storeResults.length > 0 && !loadingStore && !loadingMore && (
        <p className="skills-end-hint">{t("skills.endOfList")}</p>
      )}
    </>
  );

  const toggleEnabled = async (skill: InstalledSkill) => {
    if (!isTauri()) return;
    const next = !skill.enabled;
    try {
      await invoke("set_skill_enabled", {
        id: skill.id,
        enabled: next,
        agentId,
      });
      setInstalled((prev) =>
        prev.map((s) => (s.id === skill.id ? { ...s, enabled: next } : s)),
      );
    } catch (err) {
      setError(String(err));
    }
  };

  const toggleMachineLink = async (skill: InstalledSkill) => {
    if (!isTauri()) return;
    const next = !skill.linked;
    setLinkingId(skill.id);
    try {
      await invoke("link_machine_skill", {
        id: skill.id,
        linked: next,
        agentId,
      });
      setMachineSkills((prev) =>
        prev.map((s) => (s.id === skill.id ? { ...s, linked: next } : s)),
      );
      // 链接会在 Agent workspace 建/删 symlink，同步已安装列表供在线「已安装」判定
      void refreshInstalled({ mode: "hard" });
    } catch (err) {
      setError(String(err));
    } finally {
      setLinkingId(null);
    }
  };

  const beginInstallSkill = (skill: StoreSkill) => {
    setInstallTarget(scope === "project" ? "project" : "global");
    setInstallPromptSkill(skill);
  };

  const installSkill = async (skill: StoreSkill, target: SkillInstallTarget) => {
    if (!isTauri()) return;
    setInstallingId(skill.id);
    setError(null);
    try {
      await invoke<string>("install_store_skill", {
        installRef: skill.install_ref,
        agentId,
        name: skill.name,
        store: skill.store,
        folder: inferFolderFromInstallRef(skill.install_ref),
        scope: target,
        projectRoot: null,
      });
      setInstallPromptSkill(null);
      setScope(target);
      setPersonalTab("installed");
      showToast(
        t("skills.installDoneTarget")
          .replace("{name}", skill.name)
          .replace(
            "{target}",
            t(target === "global" ? "plugins.scope.global" : "plugins.scope.project"),
          ),
        { tone: "success" },
      );
    } catch (err) {
      setError(String(err));
      showToast(String(err), { error: true });
    } finally {
      setInstallingId(null);
    }
  };

  const installWithAgent = (skill: StoreSkill) => {
    onInstallWithAgent?.(storeInstallCommand(skill));
  };

  const updateFolderForRow = (row: SkillUpdateRow): string =>
    row.origin?.folder?.trim() ||
    inferFolderFromInstallRef(row.origin?.install_ref ?? "") ||
    row.skill.id.split(/[/\\]/).filter(Boolean).pop() ||
    row.skill.name;

  const refreshUpdatesData = async () => {
    await Promise.all([
      refreshInstalled({ mode: "hard" }),
      refreshMachine({ mode: "hard" }),
      refreshOrigins({ mode: "hard" }),
    ]);
    await loadSkillBackups();
  };

  const updateSkillRow = async (row: SkillUpdateRow) => {
    if (!isTauri() || !row.origin) return;
    const folder = updateFolderForRow(row);
    setError(null);
    try {
      const preview = await invoke<SkillUpdatePreview>("preview_skill_update", {
        folder,
        agentId,
      });
      if (preview.has_local_changes) {
        setUpdateConfirm({ mode: "single", row, folder });
        return;
      }
      await runSingleUpdate(row, folder, { force: true, backupIfDirty: true });
    } catch (err) {
      const msg = String(err);
      setError(msg);
      showToast(msg, { error: true });
    }
  };

  const invokeUpdateInstalled = (
    folder: string,
    opts: { force: boolean; backupIfDirty: boolean },
  ) =>
    invoke<string>("update_installed_skill", {
      folder,
      agentId,
      force: opts.force,
      backupIfDirty: opts.backupIfDirty,
    });

  const runSingleUpdate = async (
    row: SkillUpdateRow,
    folder: string,
    opts: { force: boolean; backupIfDirty: boolean },
  ) => {
    setUpdatingFolder(folder);
    setError(null);
    try {
      await invokeUpdateInstalled(folder, opts);
      await refreshUpdatesData();
      setLastCheckResults([]);
      showToast(t("skills.updateDone").replace("{name}", row.skill.name), {
        tone: "success",
      });
    } catch (err) {
      const msg = String(err);
      setError(msg);
      showToast(msg, { error: true });
    } finally {
      setUpdatingFolder(null);
    }
  };

  const handleSingleUpdateConfirm = (
    action: "backup" | "overwrite" | "cancel",
  ) => {
    if (!updateConfirm || updateConfirm.mode !== "single") return;
    const { row, folder } = updateConfirm;
    setUpdateConfirm(null);
    if (action === "cancel") return;
    void runSingleUpdate(row, folder, {
      force: true,
      backupIfDirty: action === "backup",
    });
  };

  const runBatchUpdate = async (targets: SkillUpdateRow[]) => {
    if (targets.length === 0) return;
    setUpdatingAll(true);
    setError(null);
    const results: SkillUpdateItemResult[] = [];
    try {
      for (const row of targets) {
        const folder = updateFolderForRow(row);
        setUpdatingFolder(folder);
        try {
          const message = await invokeUpdateInstalled(folder, {
            force: true,
            backupIfDirty: true,
          });
          results.push({ folder, ok: true, message });
        } catch (err) {
          results.push({ folder, ok: false, message: String(err) });
        }
      }
      await refreshUpdatesData();
      setLastCheckResults([]);
      const ok = results.filter((r) => r.ok).length;
      const fail = results.length - ok;
      showToast(
        t("skills.updateAllDone")
          .replace("{ok}", String(ok))
          .replace("{fail}", String(fail)),
        { tone: fail > 0 ? "warning" : "success" },
      );
    } catch (err) {
      const msg = String(err);
      setError(msg);
      showToast(msg, { error: true });
    } finally {
      setUpdatingFolder(null);
      setUpdatingAll(false);
    }
  };

  const handleBatchUpdateConfirm = (confirmed: boolean) => {
    if (!updateConfirm || updateConfirm.mode !== "batch") return;
    const { targets } = updateConfirm;
    setUpdateConfirm(null);
    if (!confirmed) return;
    void runBatchUpdate(targets);
  };

  const updateAllSkills = async () => {
    if (!isTauri()) return;
    setError(null);
    const targets = updateRows.filter(
      (row) =>
        row.status === "outdated" &&
        canUpdateSkillFromOrigin(row.skill, row.origin),
    );
    if (targets.length === 0) return;
    try {
      const previews = await Promise.all(
        targets.map(async (row) => {
          const folder = updateFolderForRow(row);
          const preview = await invoke<SkillUpdatePreview>("preview_skill_update", {
            folder,
            agentId,
          });
          return { row, preview };
        }),
      );
      const dirtyCount = previews.filter((p) => p.preview.has_local_changes).length;
      if (dirtyCount > 0) {
        setUpdateConfirm({ mode: "batch", targets, dirtyCount });
        return;
      }
      await runBatchUpdate(targets);
    } catch (err) {
      const msg = String(err);
      setError(msg);
      showToast(msg, { error: true });
    }
  };

  const flashCopied = (id: string) => {
    setCopiedId(id);
    window.setTimeout(() => {
      setCopiedId((cur) => (cur === id ? null : cur));
    }, 1600);
  };

  const copyText = async (id: string, text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      flashCopied(id);
    } catch (err) {
      setError(String(err));
    }
  };

  const copyInstalledPrompt = async (skill: InstalledSkill) => {
    const body =
      `# Skill: ${skill.name}\n\n` +
      `${skill.description || ""}\n\n` +
      `Path: ${skill.path}\n`;
    await copyText(skill.id, body.trim());
  };

  const copyStoreInstallCommand = async (skill: StoreSkill) => {
    await copyText(`${skill.id}:cmd`, storeInstallCommand(skill));
  };

  const viewSkill = async (skill: InstalledSkill | string) => {
    if (!isTauri()) return;
    const name = typeof skill === "string" ? skill : skill.name;
    const id = typeof skill === "string" ? undefined : skill.id;
    setLoadingPreview(id ?? name);
    setError(null);
    try {
      const bundle = await invoke<SkillBundle>("list_skill_bundle", {
        name,
        id: id ?? null,
      });
      const tabs = PREVIEW_TABS.filter((tab) =>
        bundle.files.some((f) => f.category === tab.id),
      );
      const firstTab = tabs[0]?.id ?? "overview";
      const firstFile =
        bundle.files.find((f) => f.category === firstTab)?.relative_path ??
        bundle.files[0]?.relative_path ??
        null;
      setPreview(bundle);
      setPreviewSkillId(id ?? null);
      setPreviewTab(firstTab);
      setPreviewFile(firstFile);
      setPreviewContent(null);
      setPreviewBinaryHint(false);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingPreview(null);
    }
  };

  const loadPreviewFile = useCallback(
    async (name: string, file: SkillFileEntry, skillId: string | null) => {
      setPreviewBinaryHint(false);
      setPreviewContent(null);
      if (!file.is_text) {
        setPreviewBinaryHint(true);
        return;
      }
      if (file.size > SKILL_PREVIEW_MAX_BYTES) {
        // SkillFileViewer 根据 size 展示大文件提示，不再请求全文
        return;
      }
      setLoadingFile(true);
      try {
        const content = await invoke<string>("get_skill_file", {
          name,
          relativePath: file.relative_path,
          id: skillId,
        });
        setPreviewContent(content);
      } catch (err) {
        setPreviewContent(String(err));
      } finally {
        setLoadingFile(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (!preview || !previewFile) return;
    const file = preview.files.find((f) => f.relative_path === previewFile);
    if (!file) return;
    void loadPreviewFile(preview.name, file, previewSkillId);
  }, [preview, previewFile, previewSkillId, loadPreviewFile]);

  useEffect(() => {
    if (!preview) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closePreview();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [preview]);

  useEffect(() => {
    if (!updateConfirm) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (updateConfirm.mode === "single") {
        handleSingleUpdateConfirm("cancel");
      } else {
        handleBatchUpdateConfirm(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [updateConfirm]);

  const previewFilesInTab = useMemo(() => {
    if (!preview) return [];
    return preview.files.filter((f) => f.category === previewTab);
  }, [preview, previewTab]);

  const availablePreviewTabs = useMemo(() => {
    if (!preview) return [];
    return PREVIEW_TABS.filter((tab) =>
      preview.files.some((f) => f.category === tab.id),
    );
  }, [preview]);

  const selectPreviewTab = (tab: SkillPreviewCategory) => {
    setPreviewTab(tab);
    const first = preview?.files.find((f) => f.category === tab);
    if (first) setPreviewFile(first.relative_path);
  };

  const closePreview = () => {
    setPreview(null);
    setPreviewSkillId(null);
    setPreviewFile(null);
    setPreviewContent(null);
    setPreviewBinaryHint(false);
  };

  const openSkillFolder = async (skill: InstalledSkill) => {
    try {
      await invoke("open_skill_folder", {
        name: skill.name,
        id: skill.id,
      });
    } catch (err) {
      setError(String(err) || t("skills.openFolderFailed"));
    }
  };

  const revealPreviewFile = async () => {
    if (!preview || !previewFile) return;
    try {
      await invoke("reveal_skill_file", {
        name: preview.name,
        relativePath: previewFile,
        id: previewSkillId,
      });
    } catch (err) {
      setError(String(err));
    }
  };

  const openPreviewFileExternal = async () => {
    if (!preview || !previewFile) return;
    try {
      await invoke("open_skill_file", {
        name: preview.name,
        relativePath: previewFile,
        id: previewSkillId,
      });
    } catch (err) {
      setError(String(err));
    }
  };

  const previewFileMeta = useMemo(() => {
    if (!preview || !previewFile) return null;
    return preview.files.find((f) => f.relative_path === previewFile) ?? null;
  }, [preview, previewFile]);

  const viewStoreDetail = async (skill: StoreSkill) => {
    const fromDetail =
      storeDetail &&
      (storeDetail.install_ref === skill.install_ref ||
        storeDetail.name === skill.name) &&
      storeDetail.detail_url?.startsWith("http")
        ? storeDetail.detail_url
        : null;
    const url = fromDetail || storeSkillDetailUrl(skill);
    if (!url) {
      setError(t("skills.detailUnavailable"));
      return;
    }
    try {
      if (isTauri()) {
        await open(url);
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch (err) {
      setError(String(err));
    }
  };

  const enabledCount = installed.filter((s) => s.enabled).length;
  const linkedCount = machineSkills.filter((s) => s.linked).length;

  /** 当前 Agent 可用：Astro 已安装，或本机技能已链接（含目录名，因 frontmatter name 常与商店名不同） */
  const availableSkillKeys = useMemo(
    () => collectInstalledSkillKeys(installed, machineSkills),
    [installed, machineSkills],
  );

  const isStoreSkillInstalled = (skill: StoreSkill) =>
    matchStoreSkillInstalled(skill, availableSkillKeys);

  const filteredInstalled = useMemo(() => {
    const q = installedQuery.trim().toLowerCase();
    let list = installed;
    if (q) {
      list = list.filter((skill) =>
        [skill.name, skill.description, skill.path, skill.source_dir, skill.id]
          .filter(Boolean)
          .some((v) => v.toLowerCase().includes(q)),
      );
    }
    if (installedSort === "calls") {
      return [...list].sort((a, b) =>
        compareByCallsThenName(
          a.name,
          b.name,
          skillCallCount(skillCalls, a),
          skillCallCount(skillCalls, b),
        ),
      );
    }
    return [...list].sort((a, b) =>
      a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
    );
  }, [installed, installedQuery, installedSort, skillCalls]);

  const filteredMachine = useMemo(() => {
    const q = machineQuery.trim().toLowerCase();
    let list = machineSkills;
    if (machineLinkFilter === "linked") {
      list = list.filter((skill) => skill.linked);
    } else if (machineLinkFilter === "unlinked") {
      list = list.filter((skill) => !skill.linked);
    }
    if (q) {
      list = list.filter((skill) =>
        [skill.name, skill.description, skill.path, skill.source_dir, skill.id]
          .filter(Boolean)
          .some((v) => v.toLowerCase().includes(q)),
      );
    }
    if (machineSort === "calls") {
      return [...list].sort((a, b) =>
        compareByCallsThenName(
          a.name,
          b.name,
          skillCallCount(skillCalls, a),
          skillCallCount(skillCalls, b),
        ),
      );
    }
    return [...list].sort((a, b) =>
      a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
    );
  }, [
    machineSkills,
    machineQuery,
    machineLinkFilter,
    machineSort,
    skillCalls,
  ]);

  const updateRows = useMemo(() => {
    const merged = mergeUpdateRows(installed, machineSkills, origins, agentId);
    return applyCheckResults(merged, lastCheckResults);
  }, [installed, machineSkills, origins, agentId, lastCheckResults]);

  const filteredUpdateRows = useMemo(
    () =>
      [...filterUpdateRows(updateRows, updateFilter)].sort((a, b) =>
        a.skill.name.toLowerCase().localeCompare(b.skill.name.toLowerCase()),
      ),
    [updateRows, updateFilter],
  );

  const outdatedCount = useMemo(
    () => updateRows.filter((row) => row.status === "outdated").length,
    [updateRows],
  );

  const agentOrigins = useMemo(
    () =>
      origins.filter(
        (origin) =>
          normalizeAgentId(origin.agent_id ?? null) === normalizeAgentId(agentId),
      ),
    [origins, agentId],
  );

  const originForSkill = useCallback(
    (skill: InstalledSkill): SkillOriginRecord | null => {
      for (const origin of agentOrigins) {
        if (originMatchesSkill(origin, skill)) return origin;
      }
      return null;
    },
    [agentOrigins],
  );

  const renderSkillUpdateButton = (skill: InstalledSkill) => {
    const origin = originForSkill(skill);
    if (!canUpdateSkillFromOrigin(skill, origin)) return null;
    const row: SkillUpdateRow = { skill, origin, status: "with_origin" };
    const folder = updateFolderForRow(row);
    const isUpdating = updatingFolder === folder || updatingAll;
    return (
      <button
        type="button"
        className="skills-action-btn"
        disabled={isUpdating}
        onClick={() => void updateSkillRow(row)}
        title={t("skills.update")}
      >
        {isUpdating ? (
          <LoaderCircle size={15} strokeWidth={2.25} className="is-spin" aria-hidden />
        ) : (
          <RefreshCw size={15} strokeWidth={2.25} aria-hidden />
        )}
        <span>{isUpdating ? t("skills.updating") : t("skills.update")}</span>
      </button>
    );
  };

  const sortedStoreResults = useMemo(() => {
    if (storeSort !== "installs") return storeResults;
    return [...storeResults].sort((a, b) => {
      const ai = a.installs ?? -1;
      const bi = b.installs ?? -1;
      if (bi !== ai) return bi - ai;
      return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    });
  }, [storeResults, storeSort]);

  const detailItems = useMemo(() => {
    if (personalTab === "machine") return filteredMachine.map((s) => s.id);
    if (personalTab === "online") return sortedStoreResults.map((s) => s.id);
    return filteredInstalled.map((s) => s.id);
  }, [personalTab, filteredInstalled, filteredMachine, sortedStoreResults]);

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedDetailId && detailItems.includes(selectedDetailId)) return;
    setSelectedDetailId(detailItems[0] ?? null);
  }, [viewMode, detailItems, selectedDetailId, personalTab, scope]);

  const selectedInstalled = filteredInstalled.find(
    (s) => s.id === selectedDetailId,
  );
  const selectedMachine = filteredMachine.find((s) => s.id === selectedDetailId);
  const selectedStore = sortedStoreResults.find((s) => s.id === selectedDetailId);

  useEffect(() => {
    if (personalTab !== "online" || !selectedDetailId || !isTauri()) {
      setStoreDetail(null);
      setLoadingStoreDetail(false);
      return;
    }
    const skill = storeResultsRef.current.find((s) => s.id === selectedDetailId);
    if (!skill) {
      setStoreDetail(null);
      setLoadingStoreDetail(false);
      return;
    }
    let cancelled = false;
    setLoadingStoreDetail(true);
    setStoreDetail(null);
    void invoke<StoreSkillDetail>("get_store_skill_detail", { skill })
      .then((detail) => {
        if (!cancelled) setStoreDetail(detail);
      })
      .catch((err) => {
        if (!cancelled) {
          setStoreDetail(null);
          setError(String(err));
        }
      })
      .finally(() => {
        if (!cancelled) setLoadingStoreDetail(false);
      });
    return () => {
      cancelled = true;
    };
  }, [personalTab, selectedDetailId]);

  const callSortOptions = useMemo(
    () => [
      { value: "name", label: t("skills.sort.name") },
      { value: "calls", label: t("skills.sort.calls") },
    ],
    [t],
  );

  const machineLinkFilterOptions = useMemo(
    () => [
      { value: "all", label: t("skills.filter.linkAll") },
      { value: "linked", label: t("skills.filter.linked") },
      { value: "unlinked", label: t("skills.filter.unlinked") },
    ],
    [t],
  );

  const storeSortOptions = useMemo(
    () => [
      { value: "default", label: t("skills.sort.default") },
      { value: "installs", label: t("skills.sort.installs") },
    ],
    [t],
  );

  const viewToggle = (
    <div
      className="skills-view-toggle"
      role="group"
      aria-label={t("skills.viewMode")}
    >
      {VIEW_OPTIONS.map(({ id, Icon, labelKey }) => (
        <button
          key={id}
          type="button"
          className={`skills-view-btn ${viewMode === id ? "is-active" : ""}`}
          onClick={() => setViewMode(id)}
          title={t(labelKey)}
          aria-label={t(labelKey)}
          aria-pressed={viewMode === id}
        >
          <Icon />
        </button>
      ))}
    </div>
  );

  const renderInstalledCard = (skill: InstalledSkill) => (
    <article
      key={skill.id}
      role="listitem"
      className={`tool-card skill-card ${skill.enabled ? "is-enabled" : "is-disabled"}`}
      data-tone={skillTone(skill.id)}
    >
      <header className="skill-card-top">
        <div className="tool-icon skill-card-icon" aria-hidden>
          <span className="tool-icon-lens" />
          <span className="tool-icon-glyph">
            <Package size={22} strokeWidth={2} />
          </span>
        </div>
        <h3 className="skill-card-title">{skill.name}</h3>
        <span className="skill-card-stat">
          {t("skills.calls", {
            n: String(skillCallCount(skillCalls, skill)),
          })}
        </span>
        <label className="skills-toggle skill-card-toggle">
          <input
            type="checkbox"
            checked={skill.enabled}
            disabled={skill.editable === false}
            onChange={() => void toggleEnabled(skill)}
          />
          <span className="skill-card-toggle-mark" aria-hidden>
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path
                d="M2 5.2 4.1 7.3 8 2.8"
                stroke="currentColor"
                strokeWidth="1.6"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </span>
          <span>
            {skill.editable === false
              ? t("plugins.readonly")
              : skill.enabled
                ? t("skills.enabled")
                : t("skills.disabled")}
          </span>
        </label>
      </header>
      <div className="skill-card-desc">
        <p>{skill.description || skill.path}</p>
        <span className="skill-card-tag" title={skill.source_dir}>
          {t(`plugins.scope.${scope}` as MessageKey)} · {formatTildePath(skill.source_dir)}
        </span>
      </div>
      <div className="skill-card-actions">
        <button
          type="button"
          className="skills-action-btn primary skill-card-primary"
          disabled={loadingPreview === skill.id || loadingPreview === skill.name}
          onClick={() => void viewSkill(skill)}
          title={
            loadingPreview === skill.id || loadingPreview === skill.name
              ? t("skills.viewing")
              : t("skills.view")
          }
        >
          {loadingPreview === skill.id || loadingPreview === skill.name ? (
            <LoaderCircle size={15} strokeWidth={2.25} className="is-spin" aria-hidden />
          ) : (
            <Eye size={15} strokeWidth={2.25} aria-hidden />
          )}
          <span>
            {loadingPreview === skill.id || loadingPreview === skill.name
              ? t("skills.viewing")
              : t("skills.view")}
          </span>
        </button>
        {renderSkillUpdateButton(skill)}
        <div className="skill-card-action-icons">
          <button
            type="button"
            className="skills-action-btn is-icon"
            onClick={() => void openSkillFolder(skill)}
            title={t("skills.openFolder")}
            aria-label={t("skills.openFolder")}
          >
            <FolderOpen size={15} strokeWidth={2.25} aria-hidden />
          </button>
          <button
            type="button"
            className="skills-action-btn is-icon"
            onClick={() => void copyInstalledPrompt(skill)}
            title={
              copiedId === skill.id ? t("skills.copied") : t("skills.copyPrompt")
            }
            aria-label={
              copiedId === skill.id ? t("skills.copied") : t("skills.copyPrompt")
            }
          >
            <CopyMorphIcon
              copied={copiedId === skill.id}
              size={15}
              aria-hidden
            />
          </button>
        </div>
      </div>
    </article>
  );

  const renderMachineCard = (skill: InstalledSkill) => (
    <article
      key={skill.id}
      role="listitem"
      className={`tool-card skill-card ${skill.linked ? "is-enabled" : "is-disabled"}`}
      data-tone={skillTone(skill.id)}
    >
      <header className="skill-card-top">
        <div className="tool-icon skill-card-icon" aria-hidden>
          <span className="tool-icon-lens" />
          <span className="tool-icon-glyph">
            <HardDrive size={22} strokeWidth={2} />
          </span>
        </div>
        <h3 className="skill-card-title">{skill.name}</h3>
        <span className="skill-card-stat">
          {t("skills.calls", {
            n: String(skillCallCount(skillCalls, skill)),
          })}
        </span>
        <span className={`skill-card-link-badge ${skill.linked ? "is-on" : ""}`}>
          {skill.linked
            ? t("skills.machineLinked")
            : t("skills.machineNotLinked")}
        </span>
      </header>
      <div className="skill-card-desc">
        <p>{skill.description || skill.path}</p>
        <span className="skill-card-tag" title={skill.source_dir}>
          {formatTildePath(skill.source_dir)}
        </span>
      </div>
      <div className="skill-card-actions">
        <button
          type="button"
          className={`skills-action-btn skill-card-primary ${skill.linked ? "" : "primary"}`}
          disabled={linkingId === skill.id}
          onClick={() => void toggleMachineLink(skill)}
          title={
            skill.linked
              ? t("skills.machineUnlink")
              : t("skills.machineLink")
          }
        >
          {linkingId === skill.id ? (
            <LoaderCircle size={15} strokeWidth={2.25} className="is-spin" aria-hidden />
          ) : skill.linked ? (
            <Unlink2 size={15} strokeWidth={2.25} aria-hidden />
          ) : (
            <Link2 size={15} strokeWidth={2.25} aria-hidden />
          )}
          <span>
            {skill.linked ? t("skills.unlink") : t("skills.link")}
          </span>
        </button>
        {renderSkillUpdateButton(skill)}
        <div className="skill-card-action-icons">
          <button
            type="button"
            className="skills-action-btn is-icon"
            disabled={loadingPreview === skill.id || loadingPreview === skill.name}
            onClick={() => void viewSkill(skill)}
            title={
              loadingPreview === skill.id || loadingPreview === skill.name
                ? t("skills.viewing")
                : t("skills.view")
            }
            aria-label={
              loadingPreview === skill.id || loadingPreview === skill.name
                ? t("skills.viewing")
                : t("skills.view")
            }
          >
            {loadingPreview === skill.id || loadingPreview === skill.name ? (
              <LoaderCircle size={15} strokeWidth={2.25} className="is-spin" aria-hidden />
            ) : (
              <Eye size={15} strokeWidth={2.25} aria-hidden />
            )}
          </button>
          <button
            type="button"
            className="skills-action-btn is-icon"
            onClick={() => void openSkillFolder(skill)}
            title={t("skills.openFolder")}
            aria-label={t("skills.openFolder")}
          >
            <FolderOpen size={15} strokeWidth={2.25} aria-hidden />
          </button>
          <button
            type="button"
            className="skills-action-btn is-icon"
            onClick={() => void copyInstalledPrompt(skill)}
            title={
              copiedId === skill.id ? t("skills.copied") : t("skills.copyPrompt")
            }
            aria-label={
              copiedId === skill.id ? t("skills.copied") : t("skills.copyPrompt")
            }
          >
            <CopyMorphIcon
              copied={copiedId === skill.id}
              size={15}
              aria-hidden
            />
          </button>
        </div>
      </div>
    </article>
  );

  const renderUpdateCard = (row: SkillUpdateRow) => {
    const { skill, origin } = row;
    const folder = updateFolderForRow(row);
    const canUpdate = canUpdateSkillFromOrigin(skill, origin);
    const isUpdating =
      updatingFolder === folder || (updatingAll && canUpdate);
    return (
      <article
        key={skill.id}
        role="listitem"
        className={`tool-card skill-card ${canUpdate ? "is-enabled" : "is-disabled"}`}
        data-tone={skillTone(skill.id)}
      >
        <header className="skill-card-top">
          <div className="tool-icon skill-card-icon" aria-hidden>
            <span className="tool-icon-lens" />
            <span className="tool-icon-glyph">
              <RefreshCw size={22} strokeWidth={2} />
            </span>
          </div>
          <h3 className="skill-card-title">{skill.name}</h3>
          {origin ? (
            <>
              {row.status === "outdated" && (
                <span
                  className="skill-card-link-badge is-outdated"
                  title={t("skills.outdatedBadge")}
                >
                  {t("skills.outdatedBadge")}
                </span>
              )}
              {row.status === "current" && (
                <span
                  className="skill-card-link-badge is-current"
                  title={t("skills.upToDate")}
                >
                  {t("skills.upToDate")}
                </span>
              )}
              <span className="skill-card-tag" title={origin.install_ref}>
                {storeBadge(origin.store)} {origin.store}
              </span>
            </>
          ) : (
            <span className="skill-card-link-badge">{t("skills.updatesFilter.noOrigin")}</span>
          )}
        </header>
        <div className="skill-card-desc">
          <p>{skill.description || skill.path}</p>
          {origin &&
            updateFilter === "with_origin" &&
            (row.status === "unknown" || row.status === "error") && (
              <p className="skill-card-status-hint">
                {t(
                  row.status === "error"
                    ? "skills.updateCheckFailed"
                    : "skills.updateStatusUnknown",
                )}
              </p>
            )}
          <span className="skill-card-tag" title={skill.source_dir}>
            {formatTildePath(skill.source_dir)}
          </span>
        </div>
        <div className="skill-card-actions">
          <button
            type="button"
            className="skills-action-btn primary skill-card-primary"
            disabled={!canUpdate || isUpdating || updatingAll}
            onClick={() => void updateSkillRow(row)}
            title={canUpdate ? t("skills.update") : t("skills.noOriginHint")}
          >
            {isUpdating ? (
              <LoaderCircle size={15} strokeWidth={2.25} className="is-spin" aria-hidden />
            ) : (
              <RefreshCw size={15} strokeWidth={2.25} aria-hidden />
            )}
            <span>{isUpdating ? t("skills.updating") : t("skills.update")}</span>
          </button>
        </div>
      </article>
    );
  };

  const renderStoreCard = (skill: StoreSkill) => {
    const already = isStoreSkillInstalled(skill);
    return (
      <article
        key={skill.id}
        role="listitem"
        className={`tool-card skill-card ${already ? "is-installed" : ""}`}
        data-tone={skillTone(skill.id)}
      >
        <header className="skill-card-top">
          <div className="tool-icon skill-card-icon" aria-hidden>
            <span className="tool-icon-lens" />
            <span className="tool-icon-glyph">
              <CloudDownload size={22} strokeWidth={2} />
            </span>
          </div>
          <h3 className="skill-card-title">{skill.name}</h3>
          {already ? (
            <span className="skill-card-link-badge is-on">
              {t("skills.alreadyInstalled")}
            </span>
          ) : skill.installs != null ? (
            <span className="skill-card-stat">
              {formatInstalls(skill.installs)}
            </span>
          ) : null}
        </header>
        <div className="skill-card-desc">
          <p>
            {storeCardDescription(skill, t("skills.detailInstalls"))}
          </p>
          <span className="skill-card-tag" title={skill.source}>
            {skill.source}
          </span>
        </div>
        <div className="skill-card-actions">
          {already ? (
            <button
              type="button"
              className="skills-action-btn skill-card-primary is-installed"
              disabled
            >
              <Check size={14} strokeWidth={2.25} aria-hidden />
              <span>{t("skills.alreadyInstalled")}</span>
            </button>
          ) : (
            <button
              type="button"
              className="skills-action-btn primary skill-card-primary"
              disabled={installingId === skill.id}
              onClick={() => beginInstallSkill(skill)}
            >
              {installingId === skill.id ? (
                <LoaderCircle
                  size={14}
                  strokeWidth={2.25}
                  className="is-spin"
                  aria-hidden
                />
              ) : (
                <Download size={14} strokeWidth={2.25} aria-hidden />
              )}
              <span>
                {installingId === skill.id
                  ? t("skills.installing")
                  : t("skills.install")}
              </span>
            </button>
          )}
          <div className="skill-card-action-icons">
            {!already ? (
              <button
                type="button"
                className="skills-action-btn is-icon"
                onClick={() => installWithAgent(skill)}
                title={t("skills.installWithAgent")}
                aria-label={t("skills.installWithAgent")}
              >
                <Bot size={15} strokeWidth={2.25} aria-hidden />
              </button>
            ) : null}
            <button
              type="button"
              className="skills-action-btn is-icon"
              onClick={() => void copyStoreInstallCommand(skill)}
              title={
                copiedId === `${skill.id}:cmd`
                  ? t("skills.copied")
                  : t("skills.copyInstallCmd")
              }
              aria-label={
                copiedId === `${skill.id}:cmd`
                  ? t("skills.copied")
                  : t("skills.copyInstallCmd")
              }
            >
              <CopyMorphIcon
                copied={copiedId === `${skill.id}:cmd`}
                size={15}
                aria-hidden
              />
            </button>
            <button
              type="button"
              className="skills-action-btn is-icon"
              onClick={() => void viewStoreDetail(skill)}
              title={t("skills.viewDetail")}
              aria-label={t("skills.viewDetail")}
            >
              <ExternalLink size={15} strokeWidth={2.25} aria-hidden />
            </button>
          </div>
        </div>
      </article>
    );
  };

  const renderInstalledDetail = () => (
    <div className="skills-detail">
      <div className="skills-detail-list" role="list">
        {filteredInstalled.map((skill) => (
          <button
            key={skill.id}
            type="button"
            role="listitem"
            className={`skills-detail-item ${selectedDetailId === skill.id ? "is-selected" : ""} ${skill.enabled ? "" : "is-disabled"}`}
            data-tone={skillTone(skill.id)}
            onClick={() => setSelectedDetailId(skill.id)}
          >
            <span className="skills-detail-item-icon" aria-hidden>
              {skill.name.slice(0, 2).toUpperCase()}
            </span>
            <span className="skills-detail-item-body">
              <span className="skills-detail-item-title">{skill.name}</span>
              <span
                className={`skills-detail-item-badge ${skill.enabled ? "is-on" : ""}`}
              >
                {skill.enabled ? t("skills.enabled") : t("skills.disabled")}
              </span>
            </span>
            <span className="skills-detail-item-calls">
              {t("skills.calls", {
                n: String(skillCallCount(skillCalls, skill)),
              })}
            </span>
          </button>
        ))}
      </div>
      <div className="skills-detail-panel">
        {selectedInstalled ? (
          <>
            <header className="skills-detail-head">
              <div>
                <div className="skills-detail-title-row">
                  <h3 className="skills-detail-title">
                    {selectedInstalled.name}
                  </h3>
                  <span className="skills-detail-item-calls is-inline">
                    {t("skills.calls", {
                      n: String(skillCallCount(skillCalls, selectedInstalled)),
                    })}
                  </span>
                </div>
                <label className="skills-toggle skills-tab-shell">
                  <input
                    type="checkbox"
                    checked={selectedInstalled.enabled}
                    disabled={selectedInstalled.editable === false}
                    onChange={() => void toggleEnabled(selectedInstalled)}
                  />
                  <span
                    className={`skills-tab-pill ${selectedInstalled.enabled ? "is-on" : ""}`}
                  >
                    <span className="skill-card-toggle-mark" aria-hidden>
                      <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
                        <path
                          d="M2 5.2 4.1 7.3 8 2.8"
                          stroke="currentColor"
                          strokeWidth="1.6"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        />
                      </svg>
                    </span>
                    <span>
                      {selectedInstalled.enabled
                        ? t("skills.enabled")
                        : t("skills.disabled")}
                    </span>
                  </span>
                </label>
              </div>
              <div className="skills-detail-actions">
                <button
                  type="button"
                  className="skills-action-btn primary"
                  disabled={
                    loadingPreview === selectedInstalled.id ||
                    loadingPreview === selectedInstalled.name
                  }
                  onClick={() => void viewSkill(selectedInstalled)}
                >
                  {loadingPreview === selectedInstalled.id ||
                  loadingPreview === selectedInstalled.name ? (
                    <LoaderCircle size={14} strokeWidth={2.25} className="is-spin" aria-hidden />
                  ) : (
                    <Eye size={14} strokeWidth={2.25} aria-hidden />
                  )}
                  {loadingPreview === selectedInstalled.id ||
                  loadingPreview === selectedInstalled.name
                    ? t("skills.viewing")
                    : t("skills.view")}
                </button>
                <div className="skills-tab-shell">
                  <button
                    type="button"
                    className="skills-action-btn"
                    onClick={() => void openSkillFolder(selectedInstalled)}
                  >
                    <FolderOpen size={14} strokeWidth={2.25} aria-hidden />
                    {t("skills.openFolder")}
                  </button>
                  <button
                    type="button"
                    className="skills-action-btn"
                    onClick={() => void copyInstalledPrompt(selectedInstalled)}
                  >
                    <CopyMorphIcon
                      copied={copiedId === selectedInstalled.id}
                      size={14}
                      aria-hidden
                    />
                    {copiedId === selectedInstalled.id
                      ? t("skills.copied")
                      : t("skills.copyPrompt")}
                  </button>
                </div>
              </div>
            </header>
            <section className="skills-detail-section">
              <h4 className="skills-detail-label">
                <FileText size={15} strokeWidth={2.25} aria-hidden />
                {t("skills.detailDescription")}
              </h4>
              <p className="skills-detail-body">
                {selectedInstalled.description || "—"}
              </p>
            </section>
            <section className="skills-detail-meta-grid">
              <div className="skills-detail-meta-item">
                <span className="skills-detail-label">
                  <FolderOpen size={15} strokeWidth={2.25} aria-hidden />
                  {t("skills.detail.path")}
                </span>
                <span title={selectedInstalled.path}>
                  {formatTildePath(selectedInstalled.path)}
                </span>
              </div>
              <div className="skills-detail-meta-item">
                <span className="skills-detail-label">
                  <Library size={15} strokeWidth={2.25} aria-hidden />
                  {t("skills.detail.source")}
                </span>
                <span title={selectedInstalled.source_dir}>
                  {formatTildePath(selectedInstalled.source_dir)}
                </span>
              </div>
            </section>
          </>
        ) : (
          <p className="skills-empty">{t("skills.detail.selectHint")}</p>
        )}
      </div>
    </div>
  );

  const renderMachineDetail = () => (
    <div className="skills-detail">
      <div className="skills-detail-list" role="list">
        {filteredMachine.map((skill) => (
          <button
            key={skill.id}
            type="button"
            role="listitem"
            className={`skills-detail-item ${selectedDetailId === skill.id ? "is-selected" : ""} ${skill.linked ? "" : "is-disabled"}`}
            data-tone={skillTone(skill.id)}
            onClick={() => setSelectedDetailId(skill.id)}
          >
            <span className="skills-detail-item-icon" aria-hidden>
              {skill.name.slice(0, 2).toUpperCase()}
            </span>
            <span className="skills-detail-item-body">
              <span className="skills-detail-item-title">{skill.name}</span>
              <span
                className={`skills-detail-item-badge ${skill.linked ? "is-on" : ""}`}
              >
                {skill.linked
                  ? t("skills.machineLinked")
                  : t("skills.machineNotLinked")}
              </span>
            </span>
            <span className="skills-detail-item-calls">
              {t("skills.calls", {
                n: String(skillCallCount(skillCalls, skill)),
              })}
            </span>
          </button>
        ))}
      </div>
      <div className="skills-detail-panel">
        {selectedMachine ? (
          <>
            <header className="skills-detail-head">
              <div>
                <div className="skills-detail-title-row">
                  <h3 className="skills-detail-title">{selectedMachine.name}</h3>
                  <span className="skills-detail-item-calls is-inline">
                    {t("skills.calls", {
                      n: String(skillCallCount(skillCalls, selectedMachine)),
                    })}
                  </span>
                </div>
                <span
                  className={`skill-card-link-badge ${selectedMachine.linked ? "is-on" : ""}`}
                >
                  {selectedMachine.linked
                    ? t("skills.machineLinked")
                    : t("skills.machineNotLinked")}
                </span>
              </div>
              <div className="skills-detail-actions">
                <button
                  type="button"
                  className="skills-action-btn primary"
                  disabled={
                    loadingPreview === selectedMachine.id ||
                    loadingPreview === selectedMachine.name
                  }
                  onClick={() => void viewSkill(selectedMachine)}
                >
                  {loadingPreview === selectedMachine.id ||
                  loadingPreview === selectedMachine.name ? (
                    <LoaderCircle size={14} strokeWidth={2.25} className="is-spin" aria-hidden />
                  ) : (
                    <Eye size={14} strokeWidth={2.25} aria-hidden />
                  )}
                  {loadingPreview === selectedMachine.id ||
                  loadingPreview === selectedMachine.name
                    ? t("skills.viewing")
                    : t("skills.view")}
                </button>
                <div className="skills-tab-shell">
                  <button
                    type="button"
                    className="skills-action-btn"
                    onClick={() => void openSkillFolder(selectedMachine)}
                  >
                    <FolderOpen size={14} strokeWidth={2.25} aria-hidden />
                    {t("skills.openFolder")}
                  </button>
                  <button
                    type="button"
                    className={`skills-action-btn ${selectedMachine.linked ? "" : "primary"}`}
                    disabled={linkingId === selectedMachine.id}
                    onClick={() => void toggleMachineLink(selectedMachine)}
                  >
                    {linkingId === selectedMachine.id ? (
                      <LoaderCircle size={14} strokeWidth={2.25} className="is-spin" aria-hidden />
                    ) : selectedMachine.linked ? (
                      <Unlink2 size={14} strokeWidth={2.25} aria-hidden />
                    ) : (
                      <Link2 size={14} strokeWidth={2.25} aria-hidden />
                    )}
                    {linkingId === selectedMachine.id
                      ? "…"
                      : selectedMachine.linked
                        ? t("skills.machineUnlink")
                        : t("skills.machineLink")}
                  </button>
                  <button
                    type="button"
                    className="skills-action-btn"
                    onClick={() => void copyInstalledPrompt(selectedMachine)}
                  >
                    <CopyMorphIcon
                      copied={copiedId === selectedMachine.id}
                      size={14}
                      aria-hidden
                    />
                    {copiedId === selectedMachine.id
                      ? t("skills.copied")
                      : t("skills.copyPrompt")}
                  </button>
                </div>
              </div>
            </header>
            <section className="skills-detail-section">
              <h4 className="skills-detail-label">
                <FileText size={15} strokeWidth={2.25} aria-hidden />
                {t("skills.detailDescription")}
              </h4>
              <p className="skills-detail-body">
                {selectedMachine.description || "—"}
              </p>
            </section>
          </>
        ) : (
          <p className="skills-empty">{t("skills.detail.selectHint")}</p>
        )}
      </div>
    </div>
  );

  const renderStoreDetail = () => (
    <div className="skills-detail">
      <div ref={detailListRef} className="skills-detail-list" role="list">
        {sortedStoreResults.map((skill) => (
          <button
            key={skill.id}
            type="button"
            role="listitem"
            className={`skills-detail-item ${selectedDetailId === skill.id ? "is-selected" : ""}`}
            data-tone={skillTone(skill.id)}
            onClick={() => setSelectedDetailId(skill.id)}
          >
            <span className="skills-detail-item-icon" aria-hidden>
              {storeBadge(skill.store)}
            </span>
            <span className="skills-detail-item-body">
              <span className="skills-detail-item-title">{skill.name}</span>
              <span className="skills-detail-item-meta">
                {isStoreSkillInstalled(skill)
                  ? t("skills.alreadyInstalled")
                  : skill.source}
              </span>
            </span>
            {skill.installs != null && (
              <span className="skills-detail-item-calls">
                {formatInstalls(skill.installs)}
              </span>
            )}
          </button>
        ))}
        <div
          ref={sentinelRef}
          className="skills-scroll-sentinel"
          aria-hidden
        />
      </div>
      <div className="skills-detail-panel">
        {selectedStore ? (
          (() => {
            const already = isStoreSkillInstalled(selectedStore);
            const detail =
              storeDetail &&
              (storeDetail.install_ref === selectedStore.install_ref ||
                storeDetail.name === selectedStore.name)
                ? storeDetail
                : null;
            const downloads = detail?.downloads ?? selectedStore.installs;
            const installs = detail?.installs ?? selectedStore.installs;
            const stars = detail?.stars ?? null;
            const author = detail?.owner_name ?? null;
            const version = detail?.version ?? null;
            const category = detail?.category ?? null;
            const updated = formatStoreUpdatedAt(detail?.updated_at, t);
            const description =
              detail?.overview?.trim() ||
              detail?.description?.trim() ||
              storeCardDescription(
                selectedStore,
                t("skills.detailInstalls"),
              );
            const storeLabel =
              selectedStore.store === "skillhub"
                ? t("skills.store.skillhub")
                : selectedStore.store === "skillsdotsh"
                  ? t("skills.store.skillsdotsh")
                  : selectedStore.store === "clawhub"
                    ? t("skills.store.clawhub")
                    : selectedStore.store;

            return (
              <>
                <header className="skills-detail-head">
                  <div>
                    <div className="skills-detail-title-row">
                      <h3 className="skills-detail-title">
                        {detail?.name || selectedStore.name}
                      </h3>
                      {downloads != null && (
                        <span className="skills-detail-item-calls is-inline">
                          {formatInstalls(downloads)}{" "}
                          {t("skills.detailDownloads")}
                        </span>
                      )}
                    </div>
                    {loadingStoreDetail && (
                      <p className="skills-detail-loading">
                        <LoaderCircle
                          size={14}
                          strokeWidth={2.25}
                          className="is-spin"
                          aria-hidden
                        />
                        {t("skills.detailLoading")}
                      </p>
                    )}
                  </div>
                  <div className="skills-detail-actions">
                    {already ? (
                      <button
                        type="button"
                        className="skills-action-btn is-installed"
                        disabled
                      >
                        <Check size={14} strokeWidth={2.25} aria-hidden />
                        {t("skills.alreadyInstalled")}
                      </button>
                    ) : (
                      <>
                        <button
                          type="button"
                          className="skills-action-btn primary"
                          disabled={installingId === selectedStore.id}
                          onClick={() => beginInstallSkill(selectedStore)}
                        >
                          {installingId === selectedStore.id ? (
                            <LoaderCircle size={14} strokeWidth={2.25} className="is-spin" aria-hidden />
                          ) : (
                            <Download size={14} strokeWidth={2.25} aria-hidden />
                          )}
                          {installingId === selectedStore.id
                            ? t("skills.installing")
                            : t("skills.install")}
                        </button>
                        <button
                          type="button"
                          className="skills-action-btn"
                          onClick={() => installWithAgent(selectedStore)}
                        >
                          <Bot size={14} strokeWidth={2.25} aria-hidden />
                          {t("skills.installWithAgent")}
                        </button>
                      </>
                    )}
                    <button
                      type="button"
                      className="skills-action-btn"
                      onClick={() => void viewStoreDetail(selectedStore)}
                    >
                      <ExternalLink size={14} strokeWidth={2.25} aria-hidden />
                      {t("skills.viewDetail")}
                    </button>
                  </div>
                </header>
                <section className="skills-detail-section">
                  <h4 className="skills-detail-label">
                    <FileText size={15} strokeWidth={2.25} aria-hidden />
                    {t("skills.detailDescription")}
                  </h4>
                  <p className="skills-detail-body">{description || "—"}</p>
                </section>
                <section className="skills-detail-meta-grid">
                  {downloads != null && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Download size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailDownloads")}
                      </span>
                      <span>{formatInstalls(downloads)}</span>
                    </div>
                  )}
                  {installs != null && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Package size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailInstalls")}
                      </span>
                      <span>{formatInstalls(installs)}</span>
                    </div>
                  )}
                  {stars != null && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Star size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailStars")}
                      </span>
                      <span>{formatInstalls(stars)}</span>
                    </div>
                  )}
                  {author && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <User size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailAuthor")}
                      </span>
                      <span title={author}>{author}</span>
                    </div>
                  )}
                  {version && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Terminal size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailVersion")}
                      </span>
                      <span>{version}</span>
                    </div>
                  )}
                  {category && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Library size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailCategory")}
                      </span>
                      <span title={category}>{category}</span>
                    </div>
                  )}
                  <div className="skills-detail-meta-item">
                    <span className="skills-detail-label">
                      <CloudDownload size={15} strokeWidth={2.25} aria-hidden />
                      {t("skills.detailStore")}
                    </span>
                    <span>{storeLabel}</span>
                  </div>
                  <div className="skills-detail-meta-item">
                    <span className="skills-detail-label">
                      <FolderOpen size={15} strokeWidth={2.25} aria-hidden />
                      {t("skills.detailSource")}
                    </span>
                    <span title={detail?.source || selectedStore.source}>
                      {detail?.source || selectedStore.source}
                    </span>
                  </div>
                  {updated && (
                    <div className="skills-detail-meta-item">
                      <span className="skills-detail-label">
                        <Sparkles size={15} strokeWidth={2.25} aria-hidden />
                        {t("skills.detailUpdated")}
                      </span>
                      <span>{updated}</span>
                    </div>
                  )}
                </section>
              </>
            );
          })()
        ) : (
          <p className="skills-empty">{t("skills.detail.selectHint")}</p>
        )}
      </div>
    </div>
  );

  return (
    <div className="skills-page" data-tone={tone ?? "indigo"} ref={pageRef}>
      <div className="plugins-primary-row">
        <div className="skills-main-tabs" role="tablist" aria-label={t("plugins.tabs")}>
          <button
            type="button"
            role="tab"
            aria-selected={primaryTab === "skills"}
            className={`skills-main-tab ${primaryTab === "skills" ? "active" : ""}`}
            onClick={() => setPrimaryTab("skills")}
          >
            <Package size={15} strokeWidth={2.25} aria-hidden />
            {t("plugins.tab.skills")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={primaryTab === "mcp"}
            className={`skills-main-tab ${primaryTab === "mcp" ? "active" : ""}`}
            onClick={() => setPrimaryTab("mcp")}
          >
            <McpIcon size={15} />
            {t("plugins.tab.mcp")}
          </button>
        </div>
      </div>

      <div className="skills-toolbar plugins-scope-toolbar">
        <div className="plugins-scope-tabs" role="tablist" aria-label={t("plugins.scopes")}>
          {(["global", "builtin", "project"] as const).map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              aria-selected={scope === item}
              className={`plugins-scope-tab ${scope === item ? "is-active" : ""}`}
              onClick={() => setScope(item)}
            >
              {item === "global" ? <Library size={14} /> : item === "builtin" ? <Bot size={14} /> : <FolderOpen size={14} />}
              {t(`plugins.scope.${item}` as MessageKey)}
            </button>
          ))}
        </div>

        <div className="skills-toolbar-end">
          {primaryTab === "skills" ? (
            <>
              {personalTab === "installed" && (
                <>
                  <SelectMenu
                    size="sm"
                    value={installedSort}
                    onChange={(v) => setInstalledSort(v as CallSort)}
                    options={callSortOptions}
                    aria-label={t("skills.sort.label")}
                  />
                  <ExpandableSearch
                    value={installedQuery}
                    onChange={setInstalledQuery}
                    placeholderKey="skills.installedSearchPlaceholder"
                  />
                </>
              )}
              {personalTab === "machine" && (
                <>
                  <SelectMenu
                    size="sm"
                    value={machineLinkFilter}
                    onChange={(value) => setMachineLinkFilter(value as MachineLinkFilter)}
                    options={machineLinkFilterOptions}
                    aria-label={t("skills.filter.linkAll")}
                  />
                  <SelectMenu
                    size="sm"
                    value={machineSort}
                    onChange={(value) => setMachineSort(value as CallSort)}
                    options={callSortOptions}
                    aria-label={t("skills.sort.label")}
                  />
                  <ExpandableSearch
                    value={machineQuery}
                    onChange={setMachineQuery}
                    placeholderKey="skills.installedSearchPlaceholder"
                  />
                </>
              )}
              {personalTab === "online" && (
                <ExpandableSearch
                  value={query}
                  onChange={setQuery}
                  onSubmit={() => void searchStore()}
                  placeholderKey="skills.searchPlaceholder"
                />
              )}
              {viewToggle}
              {scope === "global" && (
                <button type="button" className="skills-icon-btn" onClick={() => setDrawer("updates")} title={t("plugins.action.updates")} aria-label={t("plugins.action.updates")}>
                  <RefreshCw size={16} />
                  {outdatedCount > 0 && <span className="plugins-action-count">{outdatedCount}</span>}
                </button>
              )}
              <button
                type="button"
                className="skills-icon-btn"
                onClick={() => {
                  if (personalTab === "machine") {
                    void refreshMachine({ mode: "hard" });
                  } else if (personalTab === "online") {
                    void fetchStorePage(1, false, { mode: "hard" });
                  } else {
                    void refreshInstalled({ mode: "hard" });
                  }
                  void refreshSkillCalls({ force: true });
                }}
                disabled={loadingInstalled || loadingMachine || loadingStore}
                title={t("skills.refresh")}
                aria-label={t("skills.refresh")}
              >
                <IconRefresh
                  width={16}
                  height={16}
                  className={loadingInstalled || loadingMachine || loadingStore ? "is-spin" : undefined}
                />
              </button>
            </>
          ) : (
            <>
              <ExpandableSearch value={mcpQuery} onChange={setMcpQuery} placeholderKey="tools.searchPlaceholder" />
              {viewToggle}
              <button
                type="button"
                className="skills-icon-btn"
                onClick={mcp.openAdd}
                disabled={!mcp.canAdd}
                title={scope === "builtin" ? t("plugins.readonly") : t("mcpTools.add")}
                aria-label={scope === "builtin" ? t("plugins.readonly") : t("mcpTools.add")}
              >
                <CirclePlus size={17} />
              </button>
            </>
          )}
        </div>
      </div>

      {primaryTab === "skills" && scope === "global" && (
        <div
          className="plugins-personal-tabs"
          role="tablist"
          aria-label={t("plugins.personalTabs")}
        >
          {(["installed", "machine", "online"] as const).map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              aria-selected={personalTab === item}
              className={`plugins-personal-tab ${personalTab === item ? "is-active" : ""}`}
              onClick={() => setPersonalTab(item)}
            >
              {item === "installed" ? (
                <Package size={15} aria-hidden />
              ) : item === "machine" ? (
                <HardDrive size={15} aria-hidden />
              ) : (
                <CloudDownload size={15} aria-hidden />
              )}
              {t(`plugins.personalTab.${item}` as MessageKey)}
              {item === "machine" && machineSkills.length > 0 && (
                <span className="plugins-personal-tab-count">{machineSkills.length}</span>
              )}
            </button>
          ))}
        </div>
      )}

      <MotionSwitch
        switchKey={`${primaryTab}:${scope}:${personalTab}`}
        className="anim-switch--fill"
      >
      {primaryTab === "skills" && (scope !== "global" || personalTab === "installed") && (
        <section className="skills-pane" role="tabpanel">
          <header className="skills-pane-head">
            <div>
              <h2>{t(`plugins.scopeTitle.${scope}` as MessageKey)}</h2>
              <p>
                {t(`plugins.scopeSub.${scope}` as MessageKey)
                  .replace("{count}", String(enabledCount))
                  .replace("{total}", String(installed.length))}
              </p>
            </div>
          </header>

          {error && <p className="skills-error">{error}</p>}

          {viewMode === "detail" ? (
            filteredInstalled.length === 0 && !loadingInstalled ? (
              <EmptyIllustration
                scene="skills"
                size="lg"
                className="skills-empty"
                title={
                  installed.length === 0
                    ? t(`plugins.scopeEmpty.${scope}` as MessageKey)
                    : t("skills.installedSearchEmpty")
                }
              />
            ) : (
              renderInstalledDetail()
            )
          ) : (
            <div className={`skills-gallery is-${viewMode}`} role="list">
              {installed.length === 0 && !loadingInstalled && (
                <EmptyIllustration
                  scene="skills"
                  size="lg"
                  className="skills-empty"
                  title={t(`plugins.scopeEmpty.${scope}` as MessageKey)}
                  role="listitem"
                />
              )}
              {installed.length > 0 &&
                filteredInstalled.length === 0 &&
                !loadingInstalled && (
                  <EmptyIllustration
                    scene="skills"
                    size="lg"
                    className="skills-empty"
                    title={t("skills.installedSearchEmpty")}
                    role="listitem"
                  />
                )}
              {filteredInstalled.map(renderInstalledCard)}
            </div>
          )}
        </section>
      )}

      {primaryTab === "skills" && scope === "global" && personalTab === "machine" && (
        <section className="skills-pane" role="tabpanel">
          <header className="skills-pane-head">
            <div>
              <h2>{t("skills.machineTitle")}</h2>
              <p>{t("skills.machineSub")} · {linkedCount}/{machineSkills.length}</p>
            </div>
          </header>

          {error && <p className="skills-error">{error}</p>}

          {viewMode === "detail" ? (
            filteredMachine.length === 0 && !loadingMachine ? (
              <EmptyIllustration
                scene="skills"
                size="lg"
                className="skills-empty"
                title={
                  machineSkills.length === 0
                    ? t("skills.machineEmpty")
                    : t("skills.machineSearchEmpty")
                }
              />
            ) : (
              renderMachineDetail()
            )
          ) : (
            <div className={`skills-gallery is-${viewMode}`} role="list">
              {machineSkills.length === 0 && !loadingMachine && (
                <EmptyIllustration
                  scene="skills"
                  size="lg"
                  className="skills-empty"
                  title={t("skills.machineEmpty")}
                  role="listitem"
                />
              )}
              {machineSkills.length > 0 &&
                filteredMachine.length === 0 &&
                !loadingMachine && (
                  <EmptyIllustration
                    scene="skills"
                    size="lg"
                    className="skills-empty"
                    title={t("skills.machineSearchEmpty")}
                    role="listitem"
                  />
                )}
              {filteredMachine.map(renderMachineCard)}
            </div>
          )}
        </section>
      )}

      {drawer === "updates" && (
        <section className="skills-pane plugins-drawer" role="dialog" aria-modal="true">
          <header className="skills-pane-head">
            <div>
              <h2>{t("skills.updatesTitle")}</h2>
              <p>{t("skills.updatesSub")}</p>
              <p className="skills-updates-hint">
                {t("skills.updateOverwriteHint")}
              </p>
            </div>
            <div className="skills-toolbar-end">
              <button type="button" className="skills-action-btn" onClick={() => void checkSkillUpdates({ force: true })} disabled={checkingUpdates}>
                <RefreshCw size={14} />
                {t("skills.checkUpdates")}
              </button>
              <button type="button" className="skills-action-btn primary" onClick={() => void updateAllSkills()} disabled={outdatedCount === 0 || updatingAll}>
                <CloudDownload size={14} />
                {t("skills.updateAll")}
              </button>
            <button type="button" className="skills-icon-btn" onClick={() => setDrawer(null)} aria-label={t("common.close")}>
              <X size={16} />
            </button>
            </div>
          </header>

          <div className="skills-updates-toolbar">
            <div
              className="skills-update-filters"
              role="tablist"
              aria-label={t("skills.updatesTitle")}
            >
              {UPDATE_FILTERS.map(({ id, labelKey }) => (
                <button
                  key={id}
                  type="button"
                  role="tab"
                  aria-selected={updateFilter === id}
                  className={`skills-update-filter ${updateFilter === id ? "active" : ""}`}
                  onClick={() => setUpdateFilter(id)}
                >
                  {t(labelKey)}
                </button>
              ))}
            </div>
          </div>

          {error && <p className="skills-error">{error}</p>}

          <div className="skills-gallery is-list" role="list">
            {updateRows.length === 0 &&
              !loadingInstalled &&
              !loadingMachine &&
              !loadingOrigins && (
                <EmptyIllustration
                  scene="skills"
                  size="lg"
                  className="skills-empty"
                  title={t("skills.installedEmpty")}
                  role="listitem"
                />
              )}
            {updateRows.length > 0 &&
              filteredUpdateRows.length === 0 &&
              !loadingInstalled &&
              !loadingMachine &&
              !loadingOrigins && (
                <EmptyIllustration
                  scene="skills"
                  size="lg"
                  className="skills-empty"
                  title={
                    updateFilter === "updatable"
                      ? checkingUpdates
                        ? t("skills.checkingUpdates")
                        : lastCheckResults.length === 0
                          ? t("skills.updatesNeedCheck")
                          : t("skills.upToDate")
                      : t("skills.installedSearchEmpty")
                  }
                  role="listitem"
                />
              )}
            {filteredUpdateRows.map(renderUpdateCard)}
          </div>

          <div className="skills-backups">
            <button
              type="button"
              className="skills-backups-head"
              aria-expanded={backupsOpen}
              onClick={() => setBackupsOpen((open) => !open)}
            >
              <ChevronDown
                size={16}
                strokeWidth={2.25}
                className={`skills-backups-chevron ${backupsOpen ? "is-open" : ""}`}
                aria-hidden
              />
              <span>{t("skills.backupsTitle")}</span>
              {loadingBackups && (
                <LoaderCircle
                  size={14}
                  strokeWidth={2.25}
                  className="is-spin"
                  aria-hidden
                />
              )}
            </button>
            {backupsOpen && (
              <div className="skills-backups-body">
                {!loadingBackups && skillBackups.length === 0 ? (
                  <p className="skills-backups-empty">{t("skills.backupsEmpty")}</p>
                ) : (
                  <ul className="skills-backups-list">
                    {skillBackups.map((entry) => (
                      <li key={entry.path} className="skills-backup-row">
                        <span className="skills-backup-folder">{entry.folder}</span>
                        <span className="skills-backup-sep" aria-hidden>
                          ·
                        </span>
                        <span className="skills-backup-time">
                          {formatBackupTime(entry, locale)}
                        </span>
                        <button
                          type="button"
                          className="skills-action-btn"
                          title={t("skills.backupOpen")}
                          onClick={() =>
                            void invoke("reveal_skill_backup", { path: entry.path })
                          }
                        >
                          <FolderOpen size={14} strokeWidth={2.25} aria-hidden />
                          <span>{t("skills.backupOpen")}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>
        </section>
      )}

      {primaryTab === "skills" && scope === "global" && personalTab === "online" && (
        <section
          ref={onlinePaneRef}
          className={`skills-pane skills-pane-online ${viewMode === "detail" ? "is-detail" : ""}`}
          role="tabpanel"
        >
          <header className="skills-pane-head">
            <div>
              <h2>{t("skills.storeTitle")}</h2>
              <p>{t("skills.storeSub")}</p>
            </div>
          </header>

          <div className="skills-store-toolbar">
            <div className="skills-store-toolbar-row">
              <div
                className="skills-store-tabs"
                role="tablist"
                aria-label={t("skills.stores")}
              >
                {(
                  ["all", "skillhub", "skillsdotsh", "clawhub"] as const
                ).map((id) => (
                  <button
                    key={id}
                    type="button"
                    role="tab"
                    aria-selected={storeId === id}
                    className={`skills-store-tab ${storeId === id ? "active" : ""}`}
                    onClick={() => setStoreId(id)}
                  >
                    {id === "all" ? (
                      <LayoutGrid size={14} strokeWidth={2.25} aria-hidden />
                    ) : id === "skillhub" ? (
                      <Sparkles size={14} strokeWidth={2.25} aria-hidden />
                    ) : id === "clawhub" ? (
                      <Bot size={14} strokeWidth={2.25} aria-hidden />
                    ) : (
                      <Terminal size={14} strokeWidth={2.25} aria-hidden />
                    )}
                    {id === "all"
                      ? t("skills.storeAll")
                      : id === "skillhub"
                        ? t("skills.store.skillhub")
                        : id === "clawhub"
                          ? t("skills.store.clawhub")
                          : t("skills.store.skillsdotsh")}
                  </button>
                ))}
              </div>
              <SelectMenu
                size="sm"
                value={storeSort}
                onChange={(v) => setStoreSort(v as StoreSort)}
                options={storeSortOptions}
                aria-label={t("skills.sort.label")}
              />
            </div>
          </div>

          {error && <p className="skills-error">{error}</p>}

          {loadingStore ? (
            <div
              className="skills-loading-block"
              aria-busy="true"
              aria-label={t("skills.loadingMore")}
              role="status"
            >
              <MsgStreamLoader alone />
            </div>
          ) : viewMode === "detail" ? (
            storeResults.length === 0 ? (
              <EmptyIllustration
                scene="skills"
                size="lg"
                className="skills-empty"
                title={t("skills.storeEmpty")}
              />
            ) : (
              <>
                {renderStoreDetail()}
                <div className="skills-store-footer">{storeLoadMoreFooter}</div>
              </>
            )
          ) : (
            <div
              ref={galleryScrollRef}
              className={`skills-gallery is-${viewMode}`}
              role="list"
            >
              {storeResults.length === 0 ? (
                <EmptyIllustration
                  scene="skills"
                  size="lg"
                  className="skills-empty"
                  title={t("skills.storeEmpty")}
                  role="listitem"
                />
              ) : (
                sortedStoreResults.map(renderStoreCard)
              )}
              <div className="skills-store-footer" role="listitem">
                {storeLoadMoreFooter}
              </div>
            </div>
          )}
        </section>
      )}

      {primaryTab === "mcp" && (
        <section className="skills-pane" role="tabpanel">
          <div className="skills-mcp-body">{mcp.content}</div>
        </section>
      )}
      </MotionSwitch>

      {toastHost}

      {installPromptSkill &&
        createPortal(
          <div
            className="skills-install-target-backdrop"
            role="presentation"
            onClick={(event) => {
              if (event.target === event.currentTarget && !installingId) {
                setInstallPromptSkill(null);
              }
            }}
          >
            <div
              className="skills-install-target-dialog"
              role="dialog"
              aria-modal="true"
              aria-labelledby="skills-install-target-title"
            >
              <header className="skills-install-target-head">
                <span className="skills-install-target-mark" aria-hidden>
                  <Download size={19} strokeWidth={2.2} />
                </span>
                <div>
                  <p className="skills-install-target-kicker">
                    {t("skills.installTargetKicker")}
                  </p>
                  <h3 id="skills-install-target-title">{installPromptSkill.name}</h3>
                  <p>{t("skills.installTargetHint")}</p>
                </div>
                <button
                  type="button"
                  className="skills-icon-btn"
                  onClick={() => setInstallPromptSkill(null)}
                  disabled={installingId === installPromptSkill.id}
                  aria-label={t("common.close")}
                >
                  <X size={16} />
                </button>
              </header>

              <div
                className="skills-install-target-options"
                role="radiogroup"
                aria-label={t("skills.installTargetLabel")}
              >
                {(["global", "project"] as const).map((target) => (
                  <button
                    key={target}
                    type="button"
                    role="radio"
                    aria-checked={installTarget === target}
                    className={`skills-install-target-option ${installTarget === target ? "is-selected" : ""}`}
                    onClick={() => setInstallTarget(target)}
                    disabled={installingId === installPromptSkill.id}
                  >
                    <span className="skills-install-target-option-icon" aria-hidden>
                      {target === "global" ? (
                        <User size={18} strokeWidth={2.1} />
                      ) : (
                        <FolderOpen size={18} strokeWidth={2.1} />
                      )}
                    </span>
                    <span className="skills-install-target-option-copy">
                      <strong>
                        {t(
                          target === "global"
                            ? "skills.installTarget.personal"
                            : "skills.installTarget.project",
                        )}
                      </strong>
                      <small>
                        {t(
                          target === "global"
                            ? "skills.installTarget.personalDesc"
                            : "skills.installTarget.projectDesc",
                        )}
                      </small>
                      <code>
                        {target === "global"
                          ? "~/.astro/skills"
                          : "<project>/.astro/skills"}
                      </code>
                    </span>
                    <span className="skills-install-target-radio" aria-hidden>
                      <Check size={12} strokeWidth={2.8} />
                    </span>
                  </button>
                ))}
              </div>

              <footer className="skills-install-target-foot">
                <button
                  type="button"
                  className="skills-action-btn"
                  onClick={() => setInstallPromptSkill(null)}
                  disabled={installingId === installPromptSkill.id}
                >
                  {t("dialog.cancel")}
                </button>
                <button
                  type="button"
                  className="skills-action-btn primary"
                  onClick={() => void installSkill(installPromptSkill, installTarget)}
                  disabled={installingId === installPromptSkill.id}
                >
                  {installingId === installPromptSkill.id ? (
                    <LoaderCircle size={15} className="is-spin" aria-hidden />
                  ) : (
                    <Download size={15} aria-hidden />
                  )}
                  {installingId === installPromptSkill.id
                    ? t("skills.installing")
                    : t("skills.installToTarget", {
                        target: t(
                          installTarget === "global"
                            ? "plugins.scope.global"
                            : "plugins.scope.project",
                        ),
                      })}
                </button>
              </footer>
            </div>
          </div>,
          document.body,
        )}

      {updateConfirm &&
        createPortal(
          <div
            className="skills-update-confirm-backdrop"
            role="presentation"
            onClick={(e) => {
              if (e.target !== e.currentTarget) return;
              if (updateConfirm.mode === "single") {
                handleSingleUpdateConfirm("cancel");
              } else {
                handleBatchUpdateConfirm(false);
              }
            }}
          >
            <div
              className="skills-update-confirm"
              role="dialog"
              aria-modal="true"
              aria-labelledby="skills-update-confirm-title"
              onClick={(e) => e.stopPropagation()}
            >
              <h3 id="skills-update-confirm-title">
                {t("skills.updateLocalChangesTitle")}
              </h3>
              <p>
                {updateConfirm.mode === "single"
                  ? t("skills.updateLocalChangesBody")
                  : t("skills.updateBatchLocalChanges").replace(
                      "{count}",
                      String(updateConfirm.dirtyCount),
                    )}
              </p>
              <div className="skills-update-confirm-actions">
                <button
                  type="button"
                  className="skills-action-btn"
                  onClick={() =>
                    updateConfirm.mode === "single"
                      ? handleSingleUpdateConfirm("cancel")
                      : handleBatchUpdateConfirm(false)
                  }
                >
                  {t("skills.updateCancel")}
                </button>
                {updateConfirm.mode === "single" ? (
                  <button
                    type="button"
                    className="skills-action-btn"
                    onClick={() => handleSingleUpdateConfirm("overwrite")}
                  >
                    {t("skills.updateOverwriteOnly")}
                  </button>
                ) : null}
                <button
                  type="button"
                  className="skills-action-btn primary"
                  onClick={() =>
                    updateConfirm.mode === "single"
                      ? handleSingleUpdateConfirm("backup")
                      : handleBatchUpdateConfirm(true)
                  }
                >
                  {t("skills.updateBackupAndContinue")}
                </button>
              </div>
            </div>
          </div>,
          document.body,
        )}

      {preview &&
        createPortal(
          <div
            className="skills-preview-backdrop"
            data-tone="indigo"
            onClick={(e) => {
              if (e.target === e.currentTarget) closePreview();
            }}
          >
            <aside
              className="skills-preview"
              role="dialog"
              aria-modal="true"
              aria-labelledby="skills-preview-title"
            >
              <header className="skills-preview-head">
                <div className="skills-preview-head-main">
                  <div className="skills-preview-title-row">
                    <span className="skills-preview-mark" aria-hidden>
                      <span className="skills-preview-mark-lens" />
                      <Sparkles size={20} strokeWidth={2.1} />
                    </span>
                    <div className="skills-preview-title-block">
                      <p className="skills-preview-kicker">
                        {t("skills.previewKicker")}
                      </p>
                      <h3 id="skills-preview-title">{preview.name}</h3>
                      {preview.description ? (
                        <p className="skills-preview-desc">
                          {preview.description}
                        </p>
                      ) : null}
                    </div>
                  </div>
                  <div className="skills-preview-root-row">
                    <p className="skills-preview-root" title={preview.root}>
                      <HardDrive size={12} strokeWidth={2.3} aria-hidden />
                      <span>{preview.root}</span>
                    </p>
                    <button
                      type="button"
                      className="skills-preview-root-btn"
                      onClick={() =>
                        void invoke("open_skill_folder", {
                          name: preview.name,
                          id: previewSkillId,
                        }).catch((err) => setError(String(err)))
                      }
                      title={t("skills.openFolder")}
                      aria-label={t("skills.openFolder")}
                    >
                      <FolderOpen size={14} strokeWidth={2.3} aria-hidden />
                    </button>
                  </div>
                </div>
                <button
                  type="button"
                  className="skills-preview-close"
                  onClick={closePreview}
                  aria-label={t("skills.previewClose")}
                >
                  <X size={16} strokeWidth={2.5} aria-hidden />
                </button>
              </header>

              <div
                className="skills-preview-tabs"
                role="tablist"
                aria-label={t("skills.previewTabs")}
              >
                {availablePreviewTabs.map((tab) => {
                  const count = preview.files.filter(
                    (f) => f.category === tab.id,
                  ).length;
                  const TabIcon = tab.Icon;
                  return (
                    <button
                      key={tab.id}
                      type="button"
                      role="tab"
                      aria-selected={previewTab === tab.id}
                      className={`skills-preview-tab ${previewTab === tab.id ? "is-active" : ""}`}
                      onClick={() => selectPreviewTab(tab.id)}
                    >
                      <TabIcon size={13} strokeWidth={2.3} aria-hidden />
                      {t(tab.labelKey)}
                      <span className="skills-preview-tab-count">{count}</span>
                    </button>
                  );
                })}
              </div>

              {previewFilesInTab.length > 1 ? (
                <div className="skills-preview-filelist" role="list">
                  {previewFilesInTab.map((file) => {
                    const ft = resolveFileType(fileLabel(file.relative_path));
                    const Icon = ft.Icon;
                    return (
                      <button
                        key={file.relative_path}
                        type="button"
                        role="listitem"
                        data-kind={ft.kind}
                        className={`skills-preview-filechip ${previewFile === file.relative_path ? "is-active" : ""}`}
                        onClick={() => setPreviewFile(file.relative_path)}
                        title={file.relative_path}
                      >
                        <Icon size={13} strokeWidth={2.2} aria-hidden />
                        <span>{fileLabel(file.relative_path)}</span>
                        <small>{formatBytes(file.size)}</small>
                      </button>
                    );
                  })}
                </div>
              ) : null}

              <div className="skills-preview-body">
                {previewBinaryHint ? (
                  <div className="skills-preview-binary">
                    <p>{t("skills.previewBinary")}</p>
                    <button
                      type="button"
                      className="skills-action-btn"
                      onClick={() => void openPreviewFileExternal()}
                    >
                      <ExternalLink size={14} strokeWidth={2.2} aria-hidden />
                      {t("skills.previewOpenExternal")}
                    </button>
                  </div>
                ) : (
                  <SkillFileViewer
                    content={previewContent}
                    pathLabel={
                      previewFileMeta?.relative_path ?? previewFile ?? undefined
                    }
                    sizeLabel={
                      previewFileMeta
                        ? formatBytes(previewFileMeta.size)
                        : undefined
                    }
                    skillName={preview.name}
                    skillDescription={preview.description}
                    onReveal={
                      previewFile
                        ? () => {
                            void revealPreviewFile();
                          }
                        : undefined
                    }
                    filename={previewFile ?? "file"}
                    size={previewFileMeta?.size ?? 0}
                    loading={loadingFile}
                    onOpenExternal={() => void openPreviewFileExternal()}
                  />
                )}
              </div>
            </aside>
          </div>,
          document.body,
        )}
    </div>
  );
}
