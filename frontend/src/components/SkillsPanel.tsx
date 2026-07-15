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
  Bot,
  Check,
  CloudDownload,
  Columns2,
  Copy,
  Download,
  Eye,
  ExternalLink,
  FileText,
  FolderOpen,
  HardDrive,
  LayoutGrid,
  Library,
  Link2,
  List,
  LoaderCircle,
  Package,
  Sparkles,
  Star,
  Terminal,
  Unlink2,
  User,
  X,
} from "lucide-react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import {
  storeCardDescription,
  storeInstallCommand,
  storeSkillDetailUrl,
} from "../lib/skillInstallCommand";
import { resolveFileType } from "../lib/fileTypeIcon";
import {
  createLazyLoadGate,
  decideLazyLoad,
  pageHasMore,
  type LazyLoadGate,
} from "../lib/skillsLazyLoad";
import type { AgentInfo } from "../types/agent";
import { normalizeAgentId } from "../types/agent";
import { useAgentsChanged } from "../lib/agentsChanged";
import AgentPicker from "./AgentPicker";
import AnimatedSwitch from "./AnimatedSwitch";
import ExpandableSearch from "./ExpandableSearch";
import { IconRefresh } from "./NavIcons";
import { SelectMenu } from "./SelectMenu";
import {
  SkillFileViewer,
  SKILL_PREVIEW_MAX_BYTES,
} from "./SkillFileViewer";
import type {
  InstalledSkill,
  SkillBundle,
  SkillFileEntry,
  StoreSkill,
  StoreSkillDetail,
  SkillStoreId,
} from "../types";

type SkillPreviewCategory =
  | "overview"
  | "scripts"
  | "references"
  | "assets"
  | "other";

const PREVIEW_TABS: {
  id: SkillPreviewCategory;
  labelKey: MessageKey;
}[] = [
  { id: "overview", labelKey: "skills.previewTab.overview" },
  { id: "scripts", labelKey: "skills.previewTab.scripts" },
  { id: "references", labelKey: "skills.previewTab.references" },
  { id: "assets", labelKey: "skills.previewTab.assets" },
  { id: "other", labelKey: "skills.previewTab.other" },
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
type Props = {
  /** 面板是否可见（用于懒加载 / 刷新） */
  active: boolean;
  /** 跳转对话并用 Agent 安装（填入安装 Prompt） */
  onInstallWithAgent?: (prompt: string) => void;
};

/** 顶栏 Tab：已安装 / 本机 / 商店 */
type SkillsTab = "installed" | "machine" | "online";
/** 内容布局：画廊 / 列表 / 详情 */
type SkillsView = "gallery" | "list" | "detail";
/** 已安装列表排序 */
type CallSort = "name" | "calls";
/** 本机技能链接过滤 */
type MachineLinkFilter = "all" | "linked" | "unlinked";
/** 商店列表排序 */
type StoreSort = "default" | "installs";

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
  return store === "skillhub" ? "SH" : "S·";
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

export default function SkillsPanel({ active, onInstallWithAgent }: Props) {
  const { t } = useI18n();
  const [tab, setTab] = useState<SkillsTab>("installed");
  const [installed, setInstalled] = useState<InstalledSkill[]>([]);
  const [machineSkills, setMachineSkills] = useState<InstalledSkill[]>([]);
  const [storeResults, setStoreResults] = useState<StoreSkill[]>([]);
  const [storeId, setStoreId] = useState<SkillStoreId | "all">("all");
  const [query, setQuery] = useState("");
  const [installedQuery, setInstalledQuery] = useState("");
  const [machineQuery, setMachineQuery] = useState("");
  const [loadingInstalled, setLoadingInstalled] = useState(false);
  const [loadingMachine, setLoadingMachine] = useState(false);
  const [loadingStore, setLoadingStore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [storePage, setStorePage] = useState(1);
  const [hasMore, setHasMore] = useState(true);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [linkingId, setLinkingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [installMsg, setInstallMsg] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agentId, setAgentId] = useState("workspace");
  const [viewMode, setViewMode] = useState<SkillsView>(() => readSkillsView());
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

  const loadMoreLock = useRef(false);
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

  useEffect(() => {
    try {
      localStorage.setItem(SKILLS_VIEW_KEY, viewMode);
    } catch {
      // ignore
    }
  }, [viewMode]);

  const refreshInstalled = useCallback(async () => {
    if (!isTauri()) {
      setInstalled([]);
      return;
    }
    setLoadingInstalled(true);
    setError(null);
    try {
      const list = await invoke<InstalledSkill[]>("list_installed_skills", {
        agentId,
        scope: "astro",
      });
      setInstalled(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingInstalled(false);
    }
  }, [agentId]);

  const refreshMachine = useCallback(async () => {
    if (!isTauri()) {
      setMachineSkills([]);
      return;
    }
    setLoadingMachine(true);
    setError(null);
    try {
      const list = await invoke<InstalledSkill[]>("list_installed_skills", {
        agentId,
        scope: "machine",
      });
      setMachineSkills(list);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoadingMachine(false);
    }
  }, [agentId]);

  const refreshSkillCalls = useCallback(async () => {
    if (!isTauri()) {
      setSkillCalls({});
      return;
    }
    try {
      const stats = await invoke<AgentUsageSummary>("get_agent_usage_stats", {
        agentId,
      });
      setSkillCalls(stats.skills ?? {});
    } catch {
      setSkillCalls({});
    }
  }, [agentId]);

  const fetchStorePage = useCallback(async (page: number, append: boolean) => {
    if (!isTauri()) return;
    if (append) {
      if (loadMoreLock.current) return;
      loadMoreLock.current = true;
      setLoadingMore(true);
    } else {
      lazyGateRef.current = createLazyLoadGate({ suppressInitial: true });
      setLoadingStore(true);
      setHasMore(true);
    }
    setError(null);
    try {
      const list = await invoke<StoreSkill[]>("search_store_skills", {
        query: queryRef.current,
        store: storeIdRef.current,
        limit: STORE_PAGE_SIZE,
        page,
      });
      if (!append) {
        storeResultsRef.current = list;
        setStoreResults(list);
        setHasMore(list.length >= STORE_PAGE_SIZE);
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
        setHasMore(pageHasMore(list.length, STORE_PAGE_SIZE, newlyAdded));
      }
      setStorePage(page);
    } catch (err) {
      setError(String(err));
      if (!append) setStoreResults([]);
      setHasMore(false);
    } finally {
      if (append) {
        setLoadingMore(false);
        loadMoreLock.current = false;
      } else {
        setLoadingStore(false);
      }
    }
  }, []);

  const searchStore = useCallback(async () => {
    await fetchStorePage(1, false);
  }, [fetchStorePage]);

  const loadMore = useCallback(async () => {
    if (!hasMore || loadingStore || loadingMore || loadMoreLock.current) return;
    await fetchStorePage(storePage + 1, true);
  }, [fetchStorePage, hasMore, loadingMore, loadingStore, storePage]);

  loadMoreRef.current = loadMore;

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(normalizeAgentId(cfg.active_agent_id));
      } catch {
        // ignore
      }
    })();
  }, [active]);

  useAgentsChanged((payload) => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        setAgents(cfg.agents);
        setAgentId(
          normalizeAgentId(cfg.active_agent_id || payload.active_agent_id),
        );
      } catch {
        // ignore
      }
    })();
  });

  const switchAgent = async (id: string) => {
    setAgentId(id);
    if (!isTauri()) return;
    try {
      await invoke("set_active_agent", { agentId: id });
    } catch {
      // ignore
    }
  };

  useEffect(() => {
    if (!active || tab !== "installed") return;
    void refreshInstalled();
    void refreshSkillCalls();
  }, [active, tab, refreshInstalled, refreshSkillCalls]);

  useEffect(() => {
    if (!active || tab !== "machine") return;
    void refreshMachine();
    void refreshSkillCalls();
  }, [active, tab, refreshMachine, refreshSkillCalls]);

  useEffect(() => {
    if (!active || tab !== "online") return;
    // 在线「已安装」依赖本机/Astro 列表：切到 online 时一并刷新
    void refreshInstalled();
    void refreshMachine();
    void fetchStorePage(1, false);
  }, [active, tab, storeId, refreshInstalled, refreshMachine, fetchStorePage]);

  useEffect(() => {
    if (!active || tab !== "online" || !hasMore) return;
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
      { root, rootMargin: "120px", threshold: 0 },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, [active, tab, hasMore, viewMode, storeResults.length]);

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
      void refreshInstalled();
    } catch (err) {
      setError(String(err));
    } finally {
      setLinkingId(null);
    }
  };

  const installSkill = async (skill: StoreSkill) => {
    if (!isTauri()) return;
    setInstallingId(skill.id);
    setInstallMsg(null);
    setError(null);
    try {
      const msg = await invoke<string>("install_store_skill", {
        installRef: skill.install_ref,
        agentId,
      });
      setInstallMsg(msg);
      await refreshInstalled();
      setTab("installed");
    } catch (err) {
      setError(String(err));
    } finally {
      setInstallingId(null);
    }
  };

  const installWithAgent = (skill: StoreSkill) => {
    onInstallWithAgent?.(storeInstallCommand(skill));
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
    const url = storeSkillDetailUrl(skill);
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

  /** 当前 Agent 可用：Astro 已安装，或本机技能已链接 */
  const availableSkillNames = useMemo(() => {
    const names = new Set<string>();
    for (const skill of installed) {
      names.add(skill.name.toLowerCase());
    }
    for (const skill of machineSkills) {
      if (skill.linked) names.add(skill.name.toLowerCase());
    }
    return names;
  }, [installed, machineSkills]);

  const isStoreSkillInstalled = (skill: StoreSkill) =>
    availableSkillNames.has(skill.name.toLowerCase());

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
    if (tab === "installed") return filteredInstalled.map((s) => s.id);
    if (tab === "machine") return filteredMachine.map((s) => s.id);
    return sortedStoreResults.map((s) => s.id);
  }, [tab, filteredInstalled, filteredMachine, sortedStoreResults]);

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedDetailId && detailItems.includes(selectedDetailId)) return;
    setSelectedDetailId(detailItems[0] ?? null);
  }, [viewMode, detailItems, selectedDetailId, tab]);

  const selectedInstalled = filteredInstalled.find(
    (s) => s.id === selectedDetailId,
  );
  const selectedMachine = filteredMachine.find((s) => s.id === selectedDetailId);
  const selectedStore = sortedStoreResults.find((s) => s.id === selectedDetailId);

  useEffect(() => {
    if (tab !== "online" || !selectedDetailId || !isTauri()) {
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
  }, [tab, selectedDetailId]);

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
            {skill.enabled ? t("skills.enabled") : t("skills.disabled")}
          </span>
        </label>
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
          className="skills-action-btn is-icon primary"
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
          {copiedId === skill.id ? (
            <Check size={15} strokeWidth={2.25} aria-hidden />
          ) : (
            <Copy size={15} strokeWidth={2.25} aria-hidden />
          )}
        </button>
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
          className={`skills-action-btn is-icon ${skill.linked ? "" : "primary"}`}
          disabled={linkingId === skill.id}
          onClick={() => void toggleMachineLink(skill)}
          title={
            skill.linked
              ? t("skills.machineUnlink")
              : t("skills.machineLink")
          }
          aria-label={
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
          {copiedId === skill.id ? (
            <Check size={15} strokeWidth={2.25} aria-hidden />
          ) : (
            <Copy size={15} strokeWidth={2.25} aria-hidden />
          )}
        </button>
      </div>
    </article>
  );

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
        <div className="skill-card-actions is-store">
          <div className="skill-card-action-row is-primary">
            {already ? (
              <button
                type="button"
                className="skills-action-btn skill-card-install is-installed"
                disabled
              >
                <Check size={14} strokeWidth={2.25} aria-hidden />
                {t("skills.alreadyInstalled")}
              </button>
            ) : (
              <>
                <button
                  type="button"
                  className="skills-action-btn primary skill-card-install"
                  disabled={installingId === skill.id}
                  onClick={() => void installSkill(skill)}
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
                  {installingId === skill.id
                    ? t("skills.installing")
                    : t("skills.install")}
                </button>
                <button
                  type="button"
                  className="skills-action-btn is-icon"
                  onClick={() => installWithAgent(skill)}
                  title={t("skills.installWithAgent")}
                  aria-label={t("skills.installWithAgent")}
                >
                  <Bot size={15} strokeWidth={2.25} aria-hidden />
                </button>
              </>
            )}
          </div>
          <div className="skill-card-action-row is-secondary">
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
              {copiedId === `${skill.id}:cmd` ? (
                <Check size={15} strokeWidth={2.25} aria-hidden />
              ) : (
                <Copy size={15} strokeWidth={2.25} aria-hidden />
              )}
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
                <label className="skills-toggle skill-card-toggle">
                  <input
                    type="checkbox"
                    checked={selectedInstalled.enabled}
                    onChange={() => void toggleEnabled(selectedInstalled)}
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
                    {selectedInstalled.enabled
                      ? t("skills.enabled")
                      : t("skills.disabled")}
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
                  {copiedId === selectedInstalled.id ? (
                    <Check size={14} strokeWidth={2.25} aria-hidden />
                  ) : (
                    <Copy size={14} strokeWidth={2.25} aria-hidden />
                  )}
                  {copiedId === selectedInstalled.id
                    ? t("skills.copied")
                    : t("skills.copyPrompt")}
                </button>
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
                  {copiedId === selectedMachine.id ? (
                    <Check size={14} strokeWidth={2.25} aria-hidden />
                  ) : (
                    <Copy size={14} strokeWidth={2.25} aria-hidden />
                  )}
                  {copiedId === selectedMachine.id
                    ? t("skills.copied")
                    : t("skills.copyPrompt")}
                </button>
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
                          onClick={() => void installSkill(selectedStore)}
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
    <div className="skills-page" data-tone="indigo">
      <div
        className="skills-main-tabs"
        role="tablist"
        aria-label={t("skills.mainTabs")}
      >
        <button
          type="button"
          role="tab"
          aria-selected={tab === "installed"}
          className={`skills-main-tab ${tab === "installed" ? "active" : ""}`}
          onClick={() => setTab("installed")}
        >
          <Package size={15} strokeWidth={2.25} aria-hidden />
          {t("skills.tab.installed")}
          {installed.length > 0 && (
            <span className="skills-main-tab-count">
              {enabledCount}/{installed.length}
            </span>
          )}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "machine"}
          className={`skills-main-tab ${tab === "machine" ? "active" : ""}`}
          onClick={() => setTab("machine")}
        >
          <HardDrive size={15} strokeWidth={2.25} aria-hidden />
          {t("skills.tab.machine")}
          {machineSkills.length > 0 && (
            <span className="skills-main-tab-count">
              {linkedCount}/{machineSkills.length}
            </span>
          )}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "online"}
          className={`skills-main-tab ${tab === "online" ? "active" : ""}`}
          onClick={() => setTab("online")}
        >
          <CloudDownload size={15} strokeWidth={2.25} aria-hidden />
          {t("skills.tab.online")}
        </button>
      </div>

      <AnimatedSwitch switchKey={tab} className="anim-switch--fill">
      {tab === "installed" && (
        <section className="skills-pane" role="tabpanel">
          <header className="skills-pane-head">
            <div>
              <h2>{t("skills.installedTitle")}</h2>
              <p>
                {t("skills.installedSub")
                  .replace("{count}", String(enabledCount))
                  .replace("{total}", String(installed.length))}
              </p>
            </div>
            <div className="skills-pane-actions">
              <AgentPicker
                agents={agents}
                value={agentId}
                onChange={(id) => void switchAgent(id)}
                labelKey="filespace.agentFilter"
              />
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
              {viewToggle}
              <button
                type="button"
                className="skills-icon-btn"
                onClick={() => {
                  void refreshInstalled();
                  void refreshSkillCalls();
                }}
                disabled={loadingInstalled}
                title={
                  loadingInstalled ? t("skills.refreshing") : t("skills.refresh")
                }
                aria-label={
                  loadingInstalled ? t("skills.refreshing") : t("skills.refresh")
                }
              >
                <IconRefresh
                  width={16}
                  height={16}
                  className={loadingInstalled ? "is-spin" : undefined}
                />
              </button>
            </div>
          </header>

          {error && <p className="skills-error">{error}</p>}

          {viewMode === "detail" ? (
            filteredInstalled.length === 0 && !loadingInstalled ? (
              <p className="skills-empty">
                {installed.length === 0
                  ? t("skills.installedEmpty")
                  : t("skills.installedSearchEmpty")}
              </p>
            ) : (
              renderInstalledDetail()
            )
          ) : (
            <div className={`skills-gallery is-${viewMode}`} role="list">
              {installed.length === 0 && !loadingInstalled && (
                <p className="skills-empty" role="listitem">
                  {t("skills.installedEmpty")}
                </p>
              )}
              {installed.length > 0 &&
                filteredInstalled.length === 0 &&
                !loadingInstalled && (
                  <p className="skills-empty" role="listitem">
                    {t("skills.installedSearchEmpty")}
                  </p>
                )}
              {filteredInstalled.map(renderInstalledCard)}
            </div>
          )}
        </section>
      )}

      {tab === "machine" && (
        <section className="skills-pane" role="tabpanel">
          <header className="skills-pane-head">
            <div>
              <h2>{t("skills.machineTitle")}</h2>
              <p>{t("skills.machineSub")}</p>
            </div>
            <div className="skills-pane-actions">
              <AgentPicker
                agents={agents}
                value={agentId}
                onChange={(id) => void switchAgent(id)}
                labelKey="filespace.agentFilter"
              />
              <SelectMenu
                size="sm"
                value={machineLinkFilter}
                onChange={(v) => setMachineLinkFilter(v as MachineLinkFilter)}
                options={machineLinkFilterOptions}
                aria-label={t("skills.filter.linkAll")}
              />
              <SelectMenu
                size="sm"
                value={machineSort}
                onChange={(v) => setMachineSort(v as CallSort)}
                options={callSortOptions}
                aria-label={t("skills.sort.label")}
              />
              <ExpandableSearch
                value={machineQuery}
                onChange={setMachineQuery}
                placeholderKey="skills.installedSearchPlaceholder"
              />
              {viewToggle}
              <button
                type="button"
                className="skills-icon-btn"
                onClick={() => {
                  void refreshMachine();
                  void refreshSkillCalls();
                }}
                disabled={loadingMachine}
                title={
                  loadingMachine ? t("skills.refreshing") : t("skills.refresh")
                }
                aria-label={
                  loadingMachine ? t("skills.refreshing") : t("skills.refresh")
                }
              >
                <IconRefresh
                  width={16}
                  height={16}
                  className={loadingMachine ? "is-spin" : undefined}
                />
              </button>
            </div>
          </header>

          {error && <p className="skills-error">{error}</p>}

          {viewMode === "detail" ? (
            filteredMachine.length === 0 && !loadingMachine ? (
              <p className="skills-empty">
                {machineSkills.length === 0
                  ? t("skills.machineEmpty")
                  : t("skills.machineSearchEmpty")}
              </p>
            ) : (
              renderMachineDetail()
            )
          ) : (
            <div className={`skills-gallery is-${viewMode}`} role="list">
              {machineSkills.length === 0 && !loadingMachine && (
                <p className="skills-empty" role="listitem">
                  {t("skills.machineEmpty")}
                </p>
              )}
              {machineSkills.length > 0 &&
                filteredMachine.length === 0 &&
                !loadingMachine && (
                  <p className="skills-empty" role="listitem">
                    {t("skills.machineSearchEmpty")}
                  </p>
                )}
              {filteredMachine.map(renderMachineCard)}
            </div>
          )}
        </section>
      )}

      {tab === "online" && (
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
            <div className="skills-pane-actions">
              <ExpandableSearch
                value={query}
                onChange={(value) => {
                  const prev = query;
                  setQuery(value);
                  if (prev.trim() && !value.trim()) {
                    void fetchStorePage(1, false);
                  }
                }}
                onSubmit={() => void searchStore()}
                placeholderKey="skills.searchPlaceholder"
              />
              {viewToggle}
            </div>
          </header>

          <div className="skills-store-toolbar">
            <div className="skills-store-toolbar-row">
              <div
                className="skills-store-tabs"
                role="tablist"
                aria-label={t("skills.stores")}
              >
                {(["all", "skillhub", "skillsdotsh"] as const).map((id) => (
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
                    ) : (
                      <Terminal size={14} strokeWidth={2.25} aria-hidden />
                    )}
                    {id === "all"
                      ? t("skills.storeAll")
                      : id === "skillhub"
                        ? t("skills.store.skillhub")
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
          {installMsg && <p className="skills-success">{installMsg}</p>}

          {loadingStore && storeResults.length === 0 ? (
            <div className="skills-loading-block" aria-busy="true">
              <IconLoader className="is-spin skills-loader-lg" />
              <span>{t("skills.searching")}</span>
            </div>
          ) : viewMode === "detail" ? (
            storeResults.length === 0 ? (
              <p className="skills-empty">{t("skills.storeEmpty")}</p>
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
              {storeResults.length === 0 && !loadingStore && (
                <p className="skills-empty" role="listitem">
                  {t("skills.storeEmpty")}
                </p>
              )}
              {sortedStoreResults.map(renderStoreCard)}
              <div className="skills-store-footer" role="listitem">
                {storeLoadMoreFooter}
              </div>
            </div>
          )}
        </section>
      )}
      </AnimatedSwitch>

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
                <div>
                  <h3 id="skills-preview-title">
                    <FileText size={16} strokeWidth={2.3} aria-hidden />
                    {preview.name}
                  </h3>
                  {preview.description ? (
                    <p className="skills-preview-desc">{preview.description}</p>
                  ) : null}
                  <div className="skills-preview-root-row">
                    <p className="skills-preview-root" title={preview.root}>
                      {preview.root}
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
                  return (
                    <button
                      key={tab.id}
                      type="button"
                      role="tab"
                      aria-selected={previewTab === tab.id}
                      className={`skills-preview-tab ${previewTab === tab.id ? "is-active" : ""}`}
                      onClick={() => selectPreviewTab(tab.id)}
                    >
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
              ) : previewFilesInTab[0] ? (
                <div className="skills-preview-filepath">
                  <span className="skills-preview-filepath-name">
                    {previewFilesInTab[0].relative_path}
                  </span>
                  <span>{formatBytes(previewFilesInTab[0].size)}</span>
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
