/** 工具面板：内置工具开关、调用统计与审批。 */
import { useEffect, useMemo, useState, type SVGProps } from "react";
import {
  Braces,
  ChevronRight,
  Columns2,
  FileJson2,
  FileText,
  FolderPlus,
  FunctionSquare,
  Hash,
  LayoutGrid,
  List,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  Trash2,
  Type,
  Wrench,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAgentTools } from "../../hooks/providers/useAgentTools";
import { useToolLoading } from "../../hooks/providers/useToolLoading";
import { toolLoadingStatus, type ToolLoadingMode } from "../../lib/tools/toolLoading";
import type { ModelInfo } from "../../types";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useI18n } from "../../i18n/LocaleContext";
import { useConfirm } from "../../hooks/ui/DialogContext";
import type { MessageKey } from "../../i18n/messages";
import MotionSwitch from "../ui/MotionSwitch";
import ExpandableSearch from "../ui/ExpandableSearch";
import LucideByName from "../icons/LucideByName";
import { SelectMenu } from "../ui/SelectMenu";
import SecurityAuditSection from "./SecurityAuditSection";

/** 工具面板 Tab：内置 / 审批 */
type ToolTab = "builtin" | "approvals";

/** 危险命令审批设置（Tauri camelCase） */
type PermissionPreset = "ask_for_approval" | "approve_for_me" | "full_access";
type CommandTypeRule = { commandFamily: string; risk: string };
type BrowserApprovalRule = { origin: string; actionClass: string };
type ApprovalSettings = {
  preset: string;
  commandAllowlist: string[];
  commandTypeAllowlist: CommandTypeRule[];
  browserApprovalRules: BrowserApprovalRule[];
};

/** 用户级永久可写目录（Tauri camelCase） */
type PermissionWriteRoots = { profileId: string; roots: string[] };

/** 本会话已生效的额外权限（Tauri camelCase） */
type SessionPermissionGrants = {
  workspaceWrite: boolean;
  writableRoots: string[];
};

const APPROVAL_MODES: PermissionPreset[] = [
  "ask_for_approval",
  "approve_for_me",
  "full_access",
];

/** 危险命令审批设置区（全局，非按 Agent） */
function ApprovalsSection({
  active,
  sessionId = null,
}: {
  active: boolean;
  sessionId?: string | null;
}) {
  const { t } = useI18n();
  const confirm = useConfirm();
  const [settings, setSettings] = useState<ApprovalSettings | null>(null);
  const [writeRoots, setWriteRoots] = useState<PermissionWriteRoots | null>(
    null,
  );
  const [sessionGrants, setSessionGrants] =
    useState<SessionPermissionGrants | null>(null);
  const [newRoot, setNewRoot] = useState("");
  const [newEntry, setNewEntry] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        const [nextSettings, nextRoots] = await Promise.all([
          invoke<ApprovalSettings>("get_approval_settings"),
          invoke<PermissionWriteRoots>("get_permission_write_roots"),
        ]);
        setSettings(nextSettings);
        setWriteRoots(nextRoots);
      } catch (cause) {
        setError(String(cause));
      }
    })();
  }, [active]);

  // 本会话额外权限：用于「写入永久」按钮的可用状态。
  useEffect(() => {
    if (!active || !isTauri() || !sessionId) {
      setSessionGrants(null);
      return;
    }
    let cancelled = false;
    void invoke<SessionPermissionGrants>("get_session_permission_grants", {
      sessionId,
    })
      .then((grants) => {
        if (!cancelled) setSessionGrants(grants);
      })
      .catch(() => {
        if (!cancelled) setSessionGrants(null);
      });
    return () => {
      cancelled = true;
    };
  }, [active, sessionId]);

  const promotableRoots = (sessionGrants?.writableRoots ?? []).filter(
    (root) => !(writeRoots?.roots ?? []).includes(root),
  );
  // 客户端的即时提示只覆盖最确定的一条：必须是绝对路径。Astro 自身目录与敏感目录
  // 仍由后端（同一套 sanitize_write_root）判定，避免两边规则漂移。
  const newRootValue = newRoot.trim();
  const newRootError =
    newRootValue && !newRootValue.startsWith("/")
      ? t("approvals.writeRoots.needAbsolute")
      : "";

  const saveRoots = async (roots: string[]): Promise<boolean> => {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setWriteRoots(
        await invoke<PermissionWriteRoots>("set_permission_write_roots", {
          roots,
        }),
      );
      return true;
    } catch (cause) {
      setError(String(cause));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const addRoot = async () => {
    const root = newRoot.trim();
    if (!root || busy || !writeRoots) return;
    // 只在校验通过（列表真的变了）后清空输入，错误路径保留原文方便修改。
    if (await saveRoots([...writeRoots.roots, root])) setNewRoot("");
  };

  const removeRoot = async (root: string) => {
    if (busy || !writeRoots) return;
    await saveRoots(writeRoots.roots.filter((entry) => entry !== root));
  };

  const promoteSessionRoots = async () => {
    if (!sessionId || busy) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      setWriteRoots(
        await invoke<PermissionWriteRoots>("promote_session_write_roots", {
          sessionId,
        }),
      );
      setNotice(t("approvals.writeRoots.promoted"));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  const setMode = async (preset: string) => {
    if (!APPROVAL_MODES.includes(preset as PermissionPreset) || busy) return;
    let confirmed = false;
    if (preset === "full_access") {
      confirmed = await confirm({
        title: t("chat.approval.fullAccessConfirmTitle"),
        message: t("chat.approval.fullAccessConfirmMessage"),
        confirmLabel: t("chat.approval.enableFullAccess"),
        variant: "danger",
      });
      if (!confirmed) return;
    }
    setBusy(true);
    setError("");
    try {
      await invoke("set_permission_preset", { preset, confirmed });
      setSettings(await invoke<ApprovalSettings>("get_approval_settings"));
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  const addEntry = async () => {
    const entry = newEntry.trim();
    if (!entry || busy) return;
    setBusy(true);
    try {
      setSettings(
        await invoke<ApprovalSettings>("add_command_allowlist", { entry }),
      );
      setNewEntry("");
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  const removeEntry = async (entry: string) => {
    try {
      setSettings(
        await invoke<ApprovalSettings>("remove_command_allowlist", { entry }),
      );
    } catch (cause) {
      setError(String(cause));
    }
  };

  const removeTypeRule = async (rule: CommandTypeRule) => {
    try {
      setSettings(
        await invoke<ApprovalSettings>("remove_command_type_allowlist", {
          commandFamily: rule.commandFamily,
          risk: rule.risk,
        }),
      );
    } catch (cause) {
      setError(String(cause));
    }
  };

  const removeBrowserRule = async (rule: BrowserApprovalRule) => {
    try {
      setSettings(
        await invoke<ApprovalSettings>("remove_browser_approval_rule", {
          origin: rule.origin,
          actionClass: rule.actionClass,
        }),
      );
    } catch (cause) {
      setError(String(cause));
    }
  };

  if (!settings) {
    return <p className="agent-tools-empty">{t("approvals.loading")}</p>;
  }

  const modeOptions = APPROVAL_MODES.map((m) => ({
    value: m,
    label: t(
      `chat.approval.${m === "ask_for_approval" ? "askForApproval" : m === "approve_for_me" ? "approveForMe" : "fullAccess"}` as MessageKey,
    ),
  }));
  const activePreset = APPROVAL_MODES.includes(
    settings.preset as PermissionPreset,
  )
    ? settings.preset
    : "ask_for_approval";
  const modeHintKey =
    activePreset === "ask_for_approval"
      ? "askForApproval"
      : activePreset === "approve_for_me"
        ? "approveForMe"
        : "fullAccess";

  return (
    <div className="approvals-section">
      <section className="tools-detail-section approvals-mode-card">
        <h4 className="tools-detail-label">
          <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.mode.label")}
        </h4>
        <SelectMenu
          className="approvals-mode-select"
          value={activePreset}
          onChange={(v) => void setMode(v)}
          options={modeOptions}
          disabled={busy}
          aria-label={t("approvals.mode.label")}
        />
        <p className="tools-detail-body">
          {t(`chat.approval.desc.${modeHintKey}` as MessageKey)}
        </p>
        {error ? <p className="tools-detail-body is-error">{error}</p> : null}
      </section>

      <section className="tools-detail-section approvals-scope-card">
        <h4 className="tools-detail-label">
          <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.browser.label")}
        </h4>
        <p className="tools-detail-body">{t("approvals.browser.hint")}</p>
        {settings.browserApprovalRules.length === 0 ? (
          <p className="tools-detail-empty-params">
            {t("approvals.browser.empty")}
          </p>
        ) : (
          <ul className="approvals-allow-list">
            {settings.browserApprovalRules.map((rule) => (
              <li
                key={`${rule.origin}:${rule.actionClass}`}
                className="mcp-tool-row"
              >
                <span className="approvals-rule-copy">
                  <code className="mcp-tool-name">{rule.origin}</code>
                  <small className="tools-detail-body">
                    {rule.actionClass === "state_changing"
                      ? t("approvals.browser.stateChanging")
                      : rule.actionClass}
                  </small>
                </span>
                <button
                  type="button"
                  className="mcp-btn-ghost"
                  onClick={() => void removeBrowserRule(rule)}
                  aria-label={t("approvals.allowlist.remove")}
                >
                  <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                  {t("approvals.allowlist.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="tools-detail-section approvals-scope-card">
        <h4 className="tools-detail-label">
          <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.typeAllowlist.label")}
        </h4>
        <p className="tools-detail-body">{t("approvals.typeAllowlist.hint")}</p>
        {settings.commandTypeAllowlist.length === 0 ? (
          <p className="tools-detail-empty-params">
            {t("approvals.typeAllowlist.empty")}
          </p>
        ) : (
          <ul className="approvals-allow-list">
            {settings.commandTypeAllowlist.map((rule) => (
              <li
                key={`${rule.commandFamily}:${rule.risk}`}
                className="mcp-tool-row"
              >
                <span className="approvals-rule-copy">
                  <code className="mcp-tool-name">{rule.commandFamily}</code>
                  <small className="tools-detail-body">
                    {rule.risk === "dynamic shell expansion"
                      ? t("approvals.typeAllowlist.dynamic")
                      : rule.risk}
                  </small>
                </span>
                <button
                  type="button"
                  className="mcp-btn-ghost"
                  onClick={() => void removeTypeRule(rule)}
                  aria-label={t("approvals.allowlist.remove")}
                >
                  <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                  {t("approvals.allowlist.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="tools-detail-section approvals-allowlist-card">
        <h4 className="tools-detail-label">
          <Terminal size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.allowlist.label")}
        </h4>
        <p className="tools-detail-body">{t("approvals.allowlist.hint")}</p>
        <div className="approvals-add-row">
          <input
            type="text"
            value={newEntry}
            onChange={(e) => setNewEntry(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void addEntry();
            }}
            placeholder={t("approvals.allowlist.placeholder")}
          />
          <button
            type="button"
            className="mcp-btn-primary"
            onClick={() => void addEntry()}
            disabled={!newEntry.trim() || busy}
          >
            <Plus size={14} strokeWidth={2.3} aria-hidden />
            {t("approvals.allowlist.add")}
          </button>
        </div>
        {settings.commandAllowlist.length === 0 ? (
          <p className="tools-detail-empty-params">
            {t("approvals.allowlist.empty")}
          </p>
        ) : (
          <ul className="approvals-allow-list">
            {settings.commandAllowlist.map((entry) => (
              <li key={entry} className="mcp-tool-row">
                <code className="mcp-tool-name">{entry}</code>
                <button
                  type="button"
                  className="mcp-btn-ghost"
                  onClick={() => void removeEntry(entry)}
                  aria-label={t("approvals.allowlist.remove")}
                >
                  <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                  {t("approvals.allowlist.remove")}
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="tools-detail-section approvals-write-roots-card">
        <h4 className="tools-detail-label">
          <FolderPlus size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.writeRoots.label")}
        </h4>
        <p className="tools-detail-body">{t("approvals.writeRoots.hint")}</p>
        {writeRoots === null ? (
          <p className="tools-detail-empty-params">{t("approvals.loading")}</p>
        ) : (
          <>
            {writeRoots.roots.length === 0 ? (
              <p className="tools-detail-empty-params">
                {t("approvals.writeRoots.empty")}
              </p>
            ) : (
              <ul className="approvals-allow-list">
                {writeRoots.roots.map((root) => (
                  <li key={root} className="mcp-tool-row">
                    <code className="mcp-tool-name">{root}</code>
                    <button
                      type="button"
                      className="mcp-btn-ghost"
                      onClick={() => void removeRoot(root)}
                      disabled={busy}
                      aria-label={t("approvals.allowlist.remove")}
                    >
                      <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                      {t("approvals.allowlist.remove")}
                    </button>
                  </li>
                ))}
              </ul>
            )}
            <div className="approvals-add-row">
              <input
                type="text"
                value={newRoot}
                onChange={(e) => setNewRoot(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void addRoot();
                }}
                placeholder={t("approvals.writeRoots.placeholder")}
                disabled={busy}
              />
              <button
                type="button"
                className="mcp-btn-primary"
                onClick={() => void addRoot()}
                disabled={!newRootValue || Boolean(newRootError) || busy}
              >
                <Plus size={14} strokeWidth={2.3} aria-hidden />
                {t("approvals.allowlist.add")}
              </button>
            </div>
            {newRootError ? (
              <p className="tools-detail-body is-error">{newRootError}</p>
            ) : null}
            <div className="approvals-write-roots-promote">
              <button
                type="button"
                className="mcp-btn-primary"
                onClick={() => void promoteSessionRoots()}
                disabled={!sessionId || busy || promotableRoots.length === 0}
              >
                <ShieldCheck size={14} strokeWidth={2.3} aria-hidden />
                {t("approvals.writeRoots.promote")}
              </button>
              <span className="tools-detail-body">
                {!sessionId
                  ? t("approvals.writeRoots.promoteNoSession")
                  : promotableRoots.length === 0
                    ? t("approvals.writeRoots.promoteEmpty")
                    : t("approvals.writeRoots.promoteReady", {
                        n: String(promotableRoots.length),
                      })}
              </span>
            </div>
          </>
        )}
        {notice ? <p className="tools-detail-body is-ok">{notice}</p> : null}
      </section>

      <section className="tools-detail-section approvals-hardline">
        <h4 className="tools-detail-label">
          <ShieldAlert size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.hardline.label")}
        </h4>
        <p className="tools-detail-body">{t("approvals.hardline.desc")}</p>
      </section>

      <SecurityAuditSection active={active} />
    </div>
  );
}
/** 内容布局 */
type ToolsView = "gallery" | "list" | "detail";

/** 按 Agent 汇总的调用统计 */
type AgentUsageSummary = {
  agent_id: string;
  tool_total: number;
  skill_total: number;
  tools: Record<string, number>;
  skills: Record<string, number>;
};

const TOOLS_VIEW_KEY = "astro.tools.viewMode";

/** 从 localStorage 读取 Tools 视图模式 */
function readToolsView(): ToolsView {
  try {
    const v = localStorage.getItem(TOOLS_VIEW_KEY);
    if (v === "gallery" || v === "list" || v === "detail") return v;
  } catch {
    // ignore
  }
  return "gallery";
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

/** 参数类型旁的轻量字形（仅 string / int） */
function ParamTypeGlyph({ type }: { type: string }) {
  const base = type.replace(/\?$/, "").toLowerCase();
  if (base.includes("string") || base === "str") {
    return <Type size={11} strokeWidth={2.25} aria-hidden />;
  }
  if (
    base.includes("int") ||
    base === "number" ||
    base === "float" ||
    base === "double"
  ) {
    return <Hash size={11} strokeWidth={2.25} aria-hidden />;
  }
  return null;
}

const VIEW_OPTIONS = [
  {
    id: "gallery" as const,
    Icon: IconViewGallery,
    labelKey: "tools.view.gallery" as MessageKey,
  },
  {
    id: "list" as const,
    Icon: IconViewList,
    labelKey: "tools.view.list" as MessageKey,
  },
  {
    id: "detail" as const,
    Icon: IconViewDetail,
    labelKey: "tools.view.detail" as MessageKey,
  },
];

/** Tools 面板入参 */
type Props = {
  modelInfo?: ModelInfo | null;
  /** 面板是否可见（用于刷新统计） */
  active?: boolean;
  /** 打开时落到该 tab；消费后通知父级清空 */
  initialTab?: ToolTab | null;
  onInitialTabConsumed?: () => void;
  /** 当前会话；用于「把本会话额外权限写入永久」 */
  sessionId?: string | null;
};

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export default function ToolsPanel({
  modelInfo,
  active = true,
  initialTab = null,
  onInitialTabConsumed,
  sessionId = null,
}: Props) {
  const { t } = useI18n();
  const [tab, setTab] = useState<ToolTab>("builtin");
  const [search, setSearch] = useState("");
  const { activeAgentId: agentId } = useActiveAgent();
  const [viewMode, setViewMode] = useState<ToolsView>(() => readToolsView());
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [selectedFnName, setSelectedFnName] = useState<string | null>(null);
  const [usage, setUsage] = useState<AgentUsageSummary | null>(null);
  const {
    enabled,
    toggle: onToggle,
    tools: agentTools,
  } = useAgentTools(agentId);
  const query = search.trim().toLowerCase();
  const loading = useToolLoading(active);

  useEffect(() => {
    if (!initialTab) return;
    setTab(initialTab);
    onInitialTabConsumed?.();
  }, [initialTab, onInitialTabConsumed]);

  useEffect(() => {
    try {
      localStorage.setItem(TOOLS_VIEW_KEY, viewMode);
    } catch {
      // ignore
    }
  }, [viewMode]);

  useEffect(() => {
    if (!active || !isTauri()) {
      if (!active) setUsage(null);
      return;
    }
    void (async () => {
      try {
        const stats = await invoke<AgentUsageSummary>("get_agent_usage_stats", {
          agentId,
        });
        setUsage(stats);
      } catch {
        setUsage(null);
      }
    })();
  }, [active, agentId]);

  const items = useMemo(() => {
    if (tab !== "builtin") return [];
    return agentTools.filter((tool) => {
      if (!query) return true;
      const title = t(tool.titleKey).toLowerCase();
      const desc = t(tool.descKey).toLowerCase();
      const params = tool.params
        .map((p) => `${p.name} ${p.type} ${p.description ?? ""}`)
        .join(" ")
        .toLowerCase();
      const fnNames = [
        ...(tool.tools ?? []),
        tool.namespace ?? "",
        tool.registeredName ?? "",
        ...(tool.functions ?? []).flatMap((fn) => [
          fn.namespace ?? "",
          fn.registeredName ?? "",
        ]),
      ]
        .join(" ")
        .toLowerCase();
      return (
        title.includes(query) ||
        desc.includes(query) ||
        tool.id.includes(query) ||
        params.includes(query) ||
        fnNames.includes(query)
      );
    });
  }, [tab, query, t, agentTools]);

  const detailIds = useMemo<string[]>(
    () => items.map((tool) => tool.id),
    [items],
  );

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedDetailId && detailIds.includes(selectedDetailId)) return;
    setSelectedDetailId(detailIds[0] ?? null);
  }, [viewMode, detailIds, selectedDetailId]);

  const selectedTool = items.find((tool) => tool.id === selectedDetailId);

  useEffect(() => {
    if (!selectedTool) {
      setSelectedFnName(null);
      return;
    }
    const fns = selectedTool.functions ?? [];
    if (fns.length === 0) {
      setSelectedFnName(null);
      return;
    }
    setSelectedFnName((prev) =>
      prev && fns.some((f) => f.name === prev) ? prev : fns[0]!.name,
    );
  }, [selectedTool]);

  const activeFn = useMemo(() => {
    if (!selectedTool?.functions?.length) return null;
    return (
      selectedTool.functions.find((f) => f.name === selectedFnName) ??
      selectedTool.functions[0] ??
      null
    );
  }, [selectedTool, selectedFnName]);

  const detailParams = activeFn?.params ?? selectedTool?.params ?? [];
  const detailApiDesc =
    activeFn?.description ?? selectedTool?.apiDescription ?? null;
  const detailApiName =
    activeFn?.name ?? selectedTool?.tools?.[0] ?? selectedTool?.id ?? "";
  const detailNamespace = activeFn?.namespace ?? selectedTool?.namespace;
  const loadingMode = loading.settings?.adjustableToolsets.includes(selectedTool?.id ?? "")
    ? loading.settings.modes[selectedTool?.id ?? ""] ?? "auto" : "auto";
  const defaultExposure = activeFn?.exposure ?? selectedTool?.exposure;
  const detailExposure = defaultExposure;
  const detailExposureLabel =
    detailExposure === "deferred"
      ? t("loop.agentToolDeferred")
      : detailExposure === "direct"
        ? t("loop.agentToolDirect")
        : detailExposure;
  const detailRegisteredName =
    activeFn?.registeredName ?? selectedTool?.registeredName;

  return (
    <div className="agent-tools-page">
      <div className="tools-toolbar">
        <div
          className="tool-main-tabs"
          role="tablist"
          aria-label={t("tools.mainTabs")}
        >
          <button
            type="button"
            role="tab"
            aria-selected={tab === "builtin"}
            className={`tool-main-tab ${tab === "builtin" ? "active" : ""}`}
            onClick={() => setTab("builtin")}
          >
            <Wrench size={15} strokeWidth={2.25} aria-hidden />
            {t("tools.tab.builtin")}
            <span className="tool-main-tab-count">{agentTools.length}</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={tab === "approvals"}
            className={`tool-main-tab ${tab === "approvals" ? "active" : ""}`}
            onClick={() => setTab("approvals")}
          >
            <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
            {t("tools.tab.approvals")}
          </button>
        </div>

        {tab !== "approvals" && (
          <div className="tools-toolbar-end">
            <ExpandableSearch
              value={search}
              onChange={setSearch}
              placeholderKey="tools.searchPlaceholder"
            />
            <div
              className="tools-view-toggle"
              role="group"
              aria-label={t("tools.viewMode")}
            >
              {VIEW_OPTIONS.map(({ id, Icon, labelKey }) => (
                <button
                  key={id}
                  type="button"
                  className={`tools-view-btn ${viewMode === id ? "is-active" : ""}`}
                  onClick={() => setViewMode(id)}
                  title={t(labelKey)}
                  aria-label={t(labelKey)}
                  aria-pressed={viewMode === id}
                >
                  <Icon />
                </button>
              ))}
            </div>
          </div>
        )}
      </div>

      <div className="agent-tools-body">
        <MotionSwitch switchKey={tab} className="anim-switch--fill">
          {tab === "builtin" && (
            <>
              {items.length === 0 ? (
                <p className="agent-tools-empty">{t("agentTools.empty")}</p>
              ) : viewMode === "detail" ? (
                <div className="tools-detail">
                  <div className="tools-detail-list" role="list">
                    {items.map((tool) => {
                      const isOn = enabled[tool.id];
                      const callCount = usage?.tools[tool.id] ?? 0;
                      return (
                        <button
                          key={tool.id}
                          type="button"
                          role="listitem"
                          className={`tools-detail-item ${selectedDetailId === tool.id ? "is-selected" : ""} ${isOn ? "" : "is-disabled"}`}
                          onClick={() => setSelectedDetailId(tool.id)}
                        >
                          <span className="tools-detail-item-title">
                            <span className="tool-lucide-inline" aria-hidden>
                              <LucideByName
                                name={tool.emoji}
                                fallback={tool.Icon}
                                size={14}
                                strokeWidth={2}
                              />
                            </span>
                            {t(tool.titleKey)}
                            {callCount > 0 ? (
                              <span className="tools-detail-item-calls">
                                {t("tools.callCount", {
                                  n: String(callCount),
                                })}
                              </span>
                            ) : null}
                          </span>
                          <span
                            className={`tools-detail-item-badge ${isOn ? "is-on" : ""}`}
                          >
                            {isOn ? t("agentTools.on") : t("agentTools.off")}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                  <div className="tools-detail-panel">
                    {selectedTool ? (
                      <>
                        <header className="tools-detail-head">
                          <div className="tools-detail-head-main">
                            <div
                              className="tool-icon"
                              aria-hidden
                              data-tone={selectedTool.tone}
                            >
                              <span className="tool-icon-lens" />
                              <span className="tool-icon-glyph">
                                <LucideByName
                                  name={activeFn?.emoji ?? selectedTool.emoji}
                                  fallback={selectedTool.Icon}
                                  width={28}
                                  height={28}
                                  strokeWidth={2}
                                />
                              </span>
                            </div>
                            <div>
                              <h3 className="tools-detail-title">
                                {t(selectedTool.titleKey)}
                              </h3>
                              <div className="tools-detail-ids">
                                <span
                                  className="tools-detail-id-chip"
                                  title={t("tools.detail.toolset")}
                                >
                                  <span className="tools-detail-id-label">
                                    {t("tools.detail.toolset")}
                                  </span>
                                  <code>{selectedTool.id}</code>
                                </span>
                                <span
                                  className="tools-detail-id-chip"
                                  title={t("tools.detail.apiName")}
                                >
                                  <span className="tools-detail-id-label">
                                    {t("tools.detail.apiName")}
                                  </span>
                                  <code>{detailApiName}</code>
                                </span>
                                {detailNamespace ? (
                                  <span
                                    className="tools-detail-id-chip"
                                    title={t("tools.detail.namespace")}
                                  >
                                    <span className="tools-detail-id-label">
                                      {t("tools.detail.namespace")}
                                    </span>
                                    <code>{detailNamespace}</code>
                                  </span>
                                ) : null}
                                {detailExposure ? (
                                  <span
                                    className="tools-detail-id-chip"
                                    title={t("tools.loading.default")}
                                  >
                                    <span className="tools-detail-id-label">
                                      {t("tools.loading.default")}
                                    </span>
                                    <code>{detailExposureLabel}</code>
                                  </span>
                                ) : null}
                                {detailRegisteredName &&
                                detailRegisteredName !== detailApiName ? (
                                  <span
                                    className="tools-detail-id-chip"
                                    title={t("tools.detail.registeredName")}
                                  >
                                    <span className="tools-detail-id-label">
                                      {t("tools.detail.registeredName")}
                                    </span>
                                    <code>{detailRegisteredName}</code>
                                  </span>
                                ) : null}
                              </div>
                              <button
                                type="button"
                                role="switch"
                                className="tool-toggle"
                                aria-checked={!!enabled[selectedTool.id]}
                                aria-label={t("agentTools.toggleAria", {
                                  name: t(selectedTool.titleKey),
                                  state: enabled[selectedTool.id]
                                    ? t("agentTools.on")
                                    : t("agentTools.off"),
                                })}
                                onClick={() => onToggle(selectedTool.id)}
                              >
                                <span className="tool-toggle-thumb" />
                              </button>
                            </div>
                          </div>
                        </header>
                        <div className="tools-detail-scroll">
                          <section className="tools-detail-section">
                            <h4 className="tools-detail-label">{t("tools.loading.title")}</h4>
                            {loading.settings?.adjustableToolsets.includes(selectedTool.id) ? (
                              <>
                                <SelectMenu
                                  value={loadingMode}
                                  options={[
                                    { value: "auto", label: t("tools.loading.auto") },
                                    { value: "always", label: t("tools.loading.always") },
                                    { value: "on_demand", label: t("tools.loading.onDemand") },
                                  ]}
                                  aria-label={t("tools.loading.title")}
                                  disabled={loading.saving}
                                  onChange={(value) => void loading.change(selectedTool.id, value as ToolLoadingMode)}
                                />
                                <p className="tools-detail-body" role="status">
                                  {t(`tools.loading.${toolLoadingStatus(loadingMode, defaultExposure, !!enabled[selectedTool.id], modelInfo)}`)}
                                </p>
                              </>
                            ) : (
                              <p className="tools-detail-body">{t(loading.settings ? "tools.loading.fixed" : "tools.loading.unavailable")}</p>
                            )}
                            <p className="tools-detail-api-desc">{t("tools.loading.scope")}</p>
                            {loading.error && (
                              <div role="alert">
                                <p className="tools-detail-body">{loading.error}</p>
                                <button type="button" onClick={() => void loading.refresh()} disabled={loading.saving}>
                                  {t("modelRankings.retry")}
                                </button>
                              </div>
                            )}
                          </section>
                          <section className="tools-detail-section">
                            <h4 className="tools-detail-label">
                              <FileText
                                size={15}
                                strokeWidth={2.25}
                                aria-hidden
                              />
                              {t("tools.detail.description")}
                            </h4>
                            <p className="tools-detail-body">
                              {t(selectedTool.descKey)}
                            </p>
                            {detailApiDesc ? (
                              <div className="tools-detail-api-block">
                                <h4 className="tools-detail-label">
                                  <FileJson2
                                    size={15}
                                    strokeWidth={2.25}
                                    aria-hidden
                                  />
                                  {t("tools.detail.apiSchema")}
                                </h4>
                                <p className="tools-detail-api-desc">
                                  {detailApiDesc}
                                </p>
                              </div>
                            ) : null}
                          </section>
                          {(selectedTool.functions?.length ?? 0) > 0 && (
                            <section className="tools-detail-section">
                              <h4 className="tools-detail-label">
                                <FunctionSquare
                                  size={15}
                                  strokeWidth={2.25}
                                  aria-hidden
                                />
                                {t("tools.detail.functions")}
                                <span className="tools-detail-label-hint">
                                  {" "}
                                  ·{" "}
                                  {t("tools.detail.fnCount", {
                                    n: String(selectedTool.functions!.length),
                                  })}
                                </span>
                              </h4>
                              <div className="tools-fn-chips" role="tablist">
                                {selectedTool.functions!.map((fn) => (
                                  <button
                                    key={fn.name}
                                    type="button"
                                    role="tab"
                                    aria-selected={activeFn?.name === fn.name}
                                    className={`tools-fn-chip ${
                                      activeFn?.name === fn.name
                                        ? "is-active"
                                        : ""
                                    }`}
                                    onClick={() => setSelectedFnName(fn.name)}
                                    title={fn.description || fn.name}
                                  >
                                    <span
                                      className="tool-lucide-inline"
                                      aria-hidden
                                    >
                                      <LucideByName
                                        name={fn.emoji}
                                        size={14}
                                        strokeWidth={2}
                                      />
                                    </span>
                                    <code>{fn.name}</code>
                                  </button>
                                ))}
                              </div>
                              {activeFn?.description &&
                              activeFn.description.trim() !==
                                (detailApiDesc ?? "").trim() ? (
                                <p className="tools-detail-fn-desc">
                                  {activeFn.description}
                                </p>
                              ) : null}
                            </section>
                          )}
                          <section className="tools-detail-section">
                            <h4 className="tools-detail-label">
                              <Braces
                                size={15}
                                strokeWidth={2.25}
                                aria-hidden
                              />
                              {t("tools.detail.params")}
                              {activeFn ? (
                                <span className="tools-detail-label-hint">
                                  {" "}
                                  · <code>{activeFn.name}</code>
                                </span>
                              ) : null}
                            </h4>
                            {detailParams.length > 0 ? (
                              <ul className="agent-tool-params-list is-detail">
                                {detailParams.map((param) => (
                                  <li
                                    key={param.name}
                                    className="agent-tool-param is-detail"
                                  >
                                    <div className="agent-tool-param-row">
                                      <code className="agent-tool-param-name">
                                        {param.name}
                                      </code>
                                      <span className="agent-tool-param-type">
                                        <ParamTypeGlyph type={param.type} />
                                        {param.type}
                                      </span>
                                      <span
                                        className={`agent-tool-param-req ${param.optional ? "is-optional" : "is-required"}`}
                                      >
                                        {param.optional
                                          ? t("tools.detail.optional")
                                          : t("tools.detail.required")}
                                      </span>
                                    </div>
                                    {param.description ? (
                                      <p className="agent-tool-param-desc">
                                        {param.description}
                                      </p>
                                    ) : null}
                                  </li>
                                ))}
                              </ul>
                            ) : (
                              <p className="tools-detail-empty-params">
                                {t("tools.detail.noParams")}
                              </p>
                            )}
                          </section>
                        </div>
                      </>
                    ) : (
                      <p className="agent-tools-empty">
                        {t("tools.detail.selectHint")}
                      </p>
                    )}
                  </div>
                </div>
              ) : (
                <div className={`agent-tools-grid is-${viewMode}`}>
                  {items.map((tool) => {
                    const isOn = enabled[tool.id];
                    const callCount = usage?.tools[tool.id] ?? 0;
                    return (
                      <article
                        key={tool.id}
                        className={`tool-card agent-tool-card ${isOn ? "is-on" : "is-off"}`}
                        data-tone={tool.tone}
                      >
                        <header className="agent-tool-head">
                          <div className="tool-icon" aria-hidden>
                            <span className="tool-icon-lens" />
                            <span className="tool-icon-glyph">
                              <LucideByName
                                name={tool.emoji}
                                fallback={tool.Icon}
                                width={28}
                                height={28}
                                strokeWidth={2}
                              />
                            </span>
                          </div>
                          <h3 className="agent-tool-title">
                            {t(tool.titleKey)}
                          </h3>
                          {callCount > 0 ? (
                            <span className="agent-tool-call-stat">
                              {t("tools.callCount", {
                                n: String(callCount),
                              })}
                            </span>
                          ) : null}
                          <button
                            type="button"
                            role="switch"
                            className="tool-toggle"
                            aria-checked={isOn}
                            aria-label={t("agentTools.toggleAria", {
                              name: t(tool.titleKey),
                              state: isOn
                                ? t("agentTools.on")
                                : t("agentTools.off"),
                            })}
                            onClick={() => onToggle(tool.id)}
                          >
                            <span className="tool-toggle-thumb" />
                          </button>
                        </header>

                        <button
                          type="button"
                          className="tool-card-body agent-tool-detail"
                          aria-label={`${t(tool.titleKey)} · ${t("tools.view.detail")}`}
                          onClick={() => {
                            setSelectedDetailId(tool.id);
                            setViewMode("detail");
                          }}
                        >
                          <p className="agent-tool-desc">{t(tool.descKey)}</p>
                          <span className="agent-tool-card-footer">
                            {tool.params.length > 0 ? (
                              <span className="agent-tool-param-summary">
                                {t("tools.detail.params")} ·{" "}
                                {tool.params.length}
                              </span>
                            ) : null}
                            <span className="agent-tool-open-hint">
                              {t("tools.view.detail")}
                              <ChevronRight
                                size={13}
                                strokeWidth={2.25}
                                aria-hidden
                              />
                            </span>
                          </span>
                        </button>
                      </article>
                    );
                  })}
                </div>
              )}
            </>
          )}

          {tab === "approvals" && (
            <ApprovalsSection active={active} sessionId={sessionId} />
          )}
        </MotionSwitch>
      </div>
    </div>
  );
}
