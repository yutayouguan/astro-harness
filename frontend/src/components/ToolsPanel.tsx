/** 工具面板：内置工具开关、调用统计与 MCP。 */
import { useEffect, useMemo, useRef, useState, type SVGProps } from "react";
import {
  Braces,
  Columns2,
  FileText,
  FormInput,
  Hash,
  Layers,
  LayoutGrid,
  List,
  PlugZap,
  Trash2,
  Type,
  Wrench,
  X,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAgentTools } from "../hooks/useAgentTools";
import {
  parseMcpJson,
  useMcpTools,
  type McpServer,
  type McpTransportType,
} from "../hooks/useMcpTools";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import type { AgentInfo } from "../types/agent";
import { normalizeAgentId } from "../types/agent";
import { useAgentsChanged } from "../lib/agentsChanged";
import AgentPicker from "./AgentPicker";
import AnimatedSwitch from "./AnimatedSwitch";
import ExpandableSearch from "./ExpandableSearch";
import LucideByName from "./LucideByName";
import { IconRefresh } from "./NavIcons";

/** 工具面板 Tab：内置 / MCP */
type ToolTab = "builtin" | "mcp";
/** 添加 MCP 对话框：JSON 粘贴 / 表单 */
type AddTab = "json" | "form";
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
  { id: "gallery" as const, Icon: IconViewGallery, labelKey: "tools.view.gallery" as MessageKey },
  { id: "list" as const, Icon: IconViewList, labelKey: "tools.view.list" as MessageKey },
  { id: "detail" as const, Icon: IconViewDetail, labelKey: "tools.view.detail" as MessageKey },
];

const MCP_TYPES: McpTransportType[] = ["stdio", "sse", "streamableHttp"];

const MCP_TYPE_LABEL: Record<McpTransportType, MessageKey> = {
  stdio: "mcpTools.type.stdio",
  sse: "mcpTools.type.sse",
  streamableHttp: "mcpTools.type.streamableHttp",
};

/** Tools 面板入参 */
type Props = {
  /** 面板是否可见（用于刷新统计） */
  active?: boolean;
  /** 打开时落到该 tab；消费后通知父级清空 */
  initialTab?: ToolTab | null;
  onInitialTabConsumed?: () => void;
};

/** 是否运行在 Tauri 壳内 */
function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 解析 `KEY=value` 多行文本为对象（环境变量 / HTTP 头） */
function parseEnvOrHeaders(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const idx = line.indexOf("=");
    if (idx > 0) {
      out[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
    }
  }
  return out;
}

/** 新增 MCP Server 对话框 */
function McpAddDialog({
  onAdd,
  onClose,
}: {
  onAdd: (servers: McpServer[]) => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [addTab, setAddTab] = useState<AddTab>("json");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState("");

  const [formName, setFormName] = useState("");
  const [formDesc, setFormDesc] = useState("");
  const [formType, setFormType] = useState<McpTransportType>("stdio");
  const [formCommand, setFormCommand] = useState("");
  const [formArgs, setFormArgs] = useState("");
  const [formEnv, setFormEnv] = useState("");
  const [formUrl, setFormUrl] = useState("");
  const [formHeaders, setFormHeaders] = useState("");
  const [formError, setFormError] = useState("");
  const [typeMenuOpen, setTypeMenuOpen] = useState(false);
  const typeMenuRef = useRef<HTMLDivElement>(null);

  const backdropRef = useRef<HTMLDivElement>(null);
  const isStdio = formType === "stdio";

  useEffect(() => {
    if (!typeMenuOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (!typeMenuRef.current?.contains(e.target as Node)) {
        setTypeMenuOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setTypeMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [typeMenuOpen]);

  function handleBackdrop(e: React.MouseEvent) {
    if (e.target === backdropRef.current) onClose();
  }

  function handleJsonImport() {
    setJsonError("");
    try {
      const servers = parseMcpJson(jsonText.trim());
      onAdd(servers);
      onClose();
    } catch {
      setJsonError(t("mcpTools.jsonError"));
    }
  }

  function handleFormAdd() {
    setFormError("");
    if (!formName.trim()) {
      setFormError(t("mcpTools.formNameRequired"));
      return;
    }
    if (isStdio) {
      if (!formCommand.trim()) {
        setFormError(t("mcpTools.formCommandRequired"));
        return;
      }
    } else if (!formUrl.trim()) {
      setFormError(t("mcpTools.formUrlRequired"));
      return;
    }

    const args = formArgs
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);

    onAdd([
      {
        id: `mcp_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
        name: formName.trim(),
        description: formDesc.trim(),
        type: formType,
        command: isStdio ? formCommand.trim() : "",
        args: isStdio ? args : [],
        env: isStdio ? parseEnvOrHeaders(formEnv) : {},
        url: isStdio ? "" : formUrl.trim(),
        headers: isStdio ? {} : parseEnvOrHeaders(formHeaders),
        enabled: true,
        tools: {},
        discovered: [],
      },
    ]);
    onClose();
  }

  return (
    <div
      className="mcp-dialog-backdrop"
      ref={backdropRef}
      onClick={handleBackdrop}
    >
      <div className="mcp-dialog" role="dialog" aria-modal>
        <div className="mcp-dialog-head">
          <span className="mcp-dialog-title">{t("mcpTools.addTitle")}</span>
          <button
            type="button"
            className="mcp-dialog-close"
            onClick={onClose}
            aria-label={t("mcpTools.cancel")}
          >
            <X size={16} strokeWidth={2.5} aria-hidden />
          </button>
        </div>

        <div className="mcp-dialog-tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={addTab === "json"}
            className={`mcp-dialog-tab ${addTab === "json" ? "active" : ""}`}
            onClick={() => { setAddTab("json"); setJsonError(""); }}
          >
            <Braces size={14} strokeWidth={2.25} aria-hidden />
            {t("mcpTools.tabJson")}
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={addTab === "form"}
            className={`mcp-dialog-tab ${addTab === "form" ? "active" : ""}`}
            onClick={() => { setAddTab("form"); setFormError(""); }}
          >
            <FormInput size={14} strokeWidth={2.25} aria-hidden />
            {t("mcpTools.tabForm")}
          </button>
        </div>

        <AnimatedSwitch switchKey={addTab} variant="fade">
        {addTab === "json" && (
          <div className="mcp-dialog-body">
            <textarea
              className="mcp-dialog-textarea"
              value={jsonText}
              onChange={(e) => setJsonText(e.target.value)}
              placeholder={t("mcpTools.jsonPlaceholder")}
              spellCheck={false}
              rows={8}
            />
            {jsonError && (
              <p className="mcp-dialog-error">{jsonError}</p>
            )}
            <div className="mcp-dialog-actions">
              <button type="button" className="mcp-btn-ghost" onClick={onClose}>
                {t("mcpTools.cancel")}
              </button>
              <button
                type="button"
                className="mcp-btn-primary"
                onClick={handleJsonImport}
                disabled={!jsonText.trim()}
              >
                {t("mcpTools.jsonImport")}
              </button>
            </div>
          </div>
        )}

        {addTab === "form" && (
          <div className="mcp-dialog-body">
            <div className="mcp-form-grid">
              <label className="mcp-field mcp-field--full">
                <span>{t("mcpTools.formName")}</span>
                <input
                  type="text"
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder={t("mcpTools.formNamePlaceholder")}
                />
              </label>
              <label className="mcp-field mcp-field--full">
                <span>{t("mcpTools.formDesc")}</span>
                <input
                  type="text"
                  value={formDesc}
                  onChange={(e) => setFormDesc(e.target.value)}
                  placeholder={t("mcpTools.formDescPlaceholder")}
                />
              </label>
              <div className="mcp-field mcp-field--full">
                <span>{t("mcpTools.formType")}</span>
                <div className="mcp-type-picker" ref={typeMenuRef}>
                  <button
                    type="button"
                    className={`mcp-type-trigger ${typeMenuOpen ? "is-open" : ""}`}
                    aria-haspopup="listbox"
                    aria-expanded={typeMenuOpen}
                    aria-label={t("mcpTools.formType")}
                    onClick={() => setTypeMenuOpen((v) => !v)}
                  >
                    <span className="mcp-type-trigger-text">
                      {t(MCP_TYPE_LABEL[formType])}
                    </span>
                    <span className="mcp-type-chevron" aria-hidden>
                      ▾
                    </span>
                  </button>
                  {typeMenuOpen ? (
                    <ul
                      className="mcp-type-menu"
                      role="listbox"
                      aria-label={t("mcpTools.formType")}
                    >
                      {MCP_TYPES.map((type) => (
                        <li key={type} role="option" aria-selected={type === formType}>
                          <button
                            type="button"
                            className={`mcp-type-option ${type === formType ? "is-active" : ""}`}
                            onClick={() => {
                              setFormType(type);
                              setTypeMenuOpen(false);
                            }}
                          >
                            <span className="mcp-type-option-name">
                              {t(MCP_TYPE_LABEL[type])}
                            </span>
                            <span className="mcp-type-option-id">{type}</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  ) : null}
                </div>
              </div>

              {isStdio ? (
                <>
                  <label className="mcp-field mcp-field--full">
                    <span>{t("mcpTools.formCommand")}</span>
                    <input
                      type="text"
                      value={formCommand}
                      onChange={(e) => setFormCommand(e.target.value)}
                      placeholder={t("mcpTools.formCommandPlaceholder")}
                    />
                  </label>
                  <label className="mcp-field mcp-field--full">
                    <span>{t("mcpTools.formArgs")}</span>
                    <textarea
                      className="mcp-field-textarea"
                      value={formArgs}
                      onChange={(e) => setFormArgs(e.target.value)}
                      placeholder={t("mcpTools.formArgsPlaceholder")}
                      rows={3}
                    />
                  </label>
                  <label className="mcp-field mcp-field--full">
                    <span>{t("mcpTools.formEnv")}</span>
                    <textarea
                      className="mcp-field-textarea"
                      value={formEnv}
                      onChange={(e) => setFormEnv(e.target.value)}
                      placeholder={t("mcpTools.formEnvPlaceholder")}
                      rows={3}
                    />
                  </label>
                </>
              ) : (
                <>
                  <label className="mcp-field mcp-field--full">
                    <span>{t("mcpTools.formUrl")}</span>
                    <input
                      type="url"
                      value={formUrl}
                      onChange={(e) => setFormUrl(e.target.value)}
                      placeholder={t("mcpTools.formUrlPlaceholder")}
                    />
                  </label>
                  <label className="mcp-field mcp-field--full">
                    <span>{t("mcpTools.formHeaders")}</span>
                    <textarea
                      className="mcp-field-textarea"
                      value={formHeaders}
                      onChange={(e) => setFormHeaders(e.target.value)}
                      placeholder={t("mcpTools.formHeadersPlaceholder")}
                      rows={3}
                    />
                  </label>
                </>
              )}
            </div>
            {formError && (
              <p className="mcp-dialog-error">{formError}</p>
            )}
            <div className="mcp-dialog-actions">
              <button type="button" className="mcp-btn-ghost" onClick={onClose}>
                {t("mcpTools.cancel")}
              </button>
              <button
                type="button"
                className="mcp-btn-primary"
                onClick={handleFormAdd}
              >
                {t("mcpTools.formAdd")}
              </button>
            </div>
          </div>
        )}
        </AnimatedSwitch>
      </div>
    </div>
  );
}

/** MCP 卡片上展示的命令行或 URL */
function serverEndpoint(server: McpServer): string {
  if (server.type === "stdio") {
    return [server.command, ...server.args].filter(Boolean).join(" ");
  }
  return server.url || "—";
}

/** 单个 MCP Server 卡片（开关、工具列表、刷新） */
function McpServerCard({
  server,
  onToggle,
  onToggleTool,
  onRefresh,
  onRemove,
  refreshing,
}: {
  server: McpServer;
  onToggle: (id: string) => void;
  onToggleTool: (serverId: string, toolName: string) => void;
  onRefresh: (serverId: string) => void;
  onRemove: (id: string) => void;
  refreshing?: boolean;
}) {
  const { t } = useI18n();
  const headerKeys = Object.keys(server.headers ?? {});
  const envKeys = Object.keys(server.env ?? {});
  const toolRows =
    server.discovered.length > 0
      ? server.discovered
      : Object.keys(server.tools).map((name) => ({ name, description: "" }));
  return (
    <article
      className={`mcp-server-card ${server.enabled ? "is-on" : "is-off"}`}
    >
      <header className="mcp-server-head">
        <div className="mcp-server-icon" aria-hidden>
          <Layers size={22} strokeWidth={1.8} aria-hidden />
        </div>
        <div className="mcp-server-meta">
          <div className="mcp-server-title-row">
            <span className="mcp-server-name">{server.name}</span>
            <span className="mcp-server-type">{t(MCP_TYPE_LABEL[server.type])}</span>
          </div>
          <code className="mcp-server-cmd">{serverEndpoint(server)}</code>
        </div>
        <button
          type="button"
          role="switch"
          className="tool-toggle"
          aria-checked={server.enabled}
          onClick={() => onToggle(server.id)}
        >
          <span className="tool-toggle-thumb" />
        </button>
      </header>
      {server.description && (
        <p className="mcp-server-desc">{server.description}</p>
      )}
      {(envKeys.length > 0 || headerKeys.length > 0) && (
        <div className="mcp-server-env">
          {envKeys.map((k) => (
            <span key={`env-${k}`} className="mcp-server-env-key">{k}</span>
          ))}
          {headerKeys.map((k) => (
            <span key={`hdr-${k}`} className="mcp-server-env-key">{k}</span>
          ))}
        </div>
      )}
      <div className={`mcp-tool-list ${server.enabled ? "" : "is-disabled"}`}>
        <div className="mcp-tool-list-head">
          <span className="mcp-tool-list-label">{t("mcpTools.tools")}</span>
          <button
            type="button"
            className="mcp-btn-ghost mcp-refresh-btn"
            disabled={!!refreshing}
            onClick={() => onRefresh(server.id)}
          >
            <IconRefresh
              width={14}
              height={14}
              className={refreshing ? "is-spin" : undefined}
            />
            {refreshing ? t("mcpTools.refreshing") : t("mcpTools.refresh")}
          </button>
        </div>
        {toolRows.length === 0 ? (
          <p className="mcp-tool-empty">{t("mcpTools.noToolsYet")}</p>
        ) : (
          <ul className="mcp-tool-rows">
            {toolRows.map((tool) => {
              const on = server.tools[tool.name] ?? true;
              return (
                <li key={tool.name} className="mcp-tool-row">
                  <div className="mcp-tool-meta">
                    <code className="mcp-tool-name">{tool.name}</code>
                    {tool.description ? (
                      <span className="mcp-tool-desc">{tool.description}</span>
                    ) : null}
                  </div>
                  <button
                    type="button"
                    role="switch"
                    className="tool-toggle"
                    aria-checked={on}
                    disabled={!server.enabled}
                    onClick={() => onToggleTool(server.id, tool.name)}
                  >
                    <span className="tool-toggle-thumb" />
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      <button
        type="button"
        className="mcp-server-remove"
        onClick={() => onRemove(server.id)}
        aria-label={t("mcpTools.remove")}
      >
        <Trash2 size={13} strokeWidth={2.25} aria-hidden />
        {t("mcpTools.remove")}
      </button>
    </article>
  );
}

export default function ToolsPanel({
  active = true,
  initialTab = null,
  onInitialTabConsumed,
}: Props) {
  const { t } = useI18n();
  const [tab, setTab] = useState<ToolTab>("builtin");
  const [showAdd, setShowAdd] = useState(false);
  const [search, setSearch] = useState("");
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [agentId, setAgentId] = useState("workspace");
  const [viewMode, setViewMode] = useState<ToolsView>(() => readToolsView());
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [selectedFnName, setSelectedFnName] = useState<string | null>(null);
  const [usage, setUsage] = useState<AgentUsageSummary | null>(null);
  const { enabled, toggle: onToggle, tools: agentTools } = useAgentTools(agentId);
  const { servers, addServers, toggleServer, toggleTool, removeServer, refreshTools, refreshing } =
    useMcpTools(agentId);
  const query = search.trim().toLowerCase();

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
        setAgentId(normalizeAgentId(cfg.active_agent_id || payload.active_agent_id));
      } catch {
        // ignore
      }
    })();
  });

  const switchAgent = async (id: string) => {
    setAgentId(id);
    if (!isTauri()) return;
    try {
      const cfg = await invoke<{
        active_agent_id: string;
        agents: AgentInfo[];
      }>("set_active_agent", { agentId: id });
      setAgents(cfg.agents);
      setAgentId(normalizeAgentId(cfg.active_agent_id));
    } catch {
      // keep local selection
    }
  };

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
      const fnNames = (tool.tools ?? []).join(" ").toLowerCase();
      return (
        title.includes(query) ||
        desc.includes(query) ||
        tool.id.includes(query) ||
        params.includes(query) ||
        fnNames.includes(query)
      );
    });
  }, [tab, query, t, agentTools]);

  const filteredServers = useMemo(() => {
    if (!query) return servers;
    return servers.filter(
      (s) =>
        s.name.toLowerCase().includes(query) ||
        s.description.toLowerCase().includes(query) ||
        s.type.toLowerCase().includes(query) ||
        s.command.toLowerCase().includes(query) ||
        s.url.toLowerCase().includes(query) ||
        s.args.join(" ").toLowerCase().includes(query),
    );
  }, [servers, query]);

  const detailIds = useMemo(() => {
    if (tab === "builtin") return items.map((tool) => tool.id);
    return filteredServers.map((s) => s.id);
  }, [tab, items, filteredServers]);

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedDetailId && detailIds.includes(selectedDetailId)) return;
    setSelectedDetailId(detailIds[0] ?? null);
  }, [viewMode, detailIds, selectedDetailId, tab]);

  const selectedTool = items.find((tool) => tool.id === selectedDetailId);
  const selectedServer = filteredServers.find((s) => s.id === selectedDetailId);

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

  return (
    <div className="agent-tools-page">
      <div className="panel-agent-toolbar">
        <div className="panel-agent-toolbar-start">
          <AgentPicker
            agents={agents}
            value={agentId}
            onChange={(id) => void switchAgent(id)}
            labelKey="filespace.agentFilter"
          />
          <span className="tools-usage-chip" aria-live="polite">
            {t("tools.callTotal", { n: String(usage?.tool_total ?? 0) })}
          </span>
        </div>
        <div className="panel-agent-toolbar-end">
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
          {tab === "mcp" && (
            <button
              type="button"
              className="mcp-add-btn"
              onClick={() => setShowAdd(true)}
              title={t("mcpTools.add")}
              aria-label={t("mcpTools.add")}
            >
              <PlugZap size={17} strokeWidth={2.2} aria-hidden />
            </button>
          )}
        </div>
      </div>
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
          aria-selected={tab === "mcp"}
          className={`tool-main-tab ${tab === "mcp" ? "active" : ""}`}
          onClick={() => setTab("mcp")}
        >
          <Layers size={15} strokeWidth={2.25} aria-hidden />
          {t("tools.tab.mcp")}
          {servers.length > 0 && (
            <span className="tool-main-tab-count">{servers.length}</span>
          )}
        </button>
      </div>

      <AnimatedSwitch switchKey={tab} className="anim-switch--fill">
      {tab === "builtin" && (
        <>
          {items.length === 0 ? (
            <p className="agent-tools-empty">{t("agentTools.empty")}</p>
          ) : viewMode === "detail" ? (
            <div className="tools-detail">
              <div className="tools-detail-list" role="list">
                {items.map((tool) => {
                  const isOn = enabled[tool.id];
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
                        <span className="tools-detail-item-calls">
                          {t("tools.callCount", {
                            n: String(usage?.tools[tool.id] ?? 0),
                          })}
                        </span>
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
                        <div className="tool-icon" aria-hidden data-tone={selectedTool.tone}>
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
                    <section className="tools-detail-section">
                      <h4 className="tools-detail-label">
                        <FileText size={15} strokeWidth={2.25} aria-hidden />
                        {t("tools.detail.description")}
                      </h4>
                      <p className="tools-detail-body">{t(selectedTool.descKey)}</p>
                      {detailApiDesc ? (
                        <p className="tools-detail-api-desc">{detailApiDesc}</p>
                      ) : null}
                    </section>
                    {selectedTool.functions && selectedTool.functions.length > 0 && (
                      <section className="tools-detail-section">
                        <h4 className="tools-detail-label">
                          {t("tools.detail.functions")}
                        </h4>
                        <div className="tools-fn-chips" role="tablist">
                          {selectedTool.functions.map((fn) => (
                            <button
                              key={fn.name}
                              type="button"
                              role="tab"
                              aria-selected={activeFn?.name === fn.name}
                              className={`tools-fn-chip ${
                                activeFn?.name === fn.name ? "is-active" : ""
                              }`}
                              onClick={() => setSelectedFnName(fn.name)}
                            >
                              <span className="tool-lucide-inline" aria-hidden>
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
                      </section>
                    )}
                    {detailParams.length > 0 && (
                      <section className="tools-detail-section">
                        <h4 className="tools-detail-label">
                          <Braces size={15} strokeWidth={2.25} aria-hidden />
                          {t("tools.detail.params")}
                          {activeFn ? (
                            <span className="tools-detail-label-hint">
                              {" "}
                              · {activeFn.name}
                            </span>
                          ) : null}
                        </h4>
                        <ul className="agent-tool-params-list is-detail">
                          {detailParams.map((param) => (
                            <li key={param.name} className="agent-tool-param is-detail">
                              <div className="agent-tool-param-row">
                                <code className="agent-tool-param-name">
                                  {param.name}
                                </code>
                                <span className="agent-tool-param-type">
                                  <ParamTypeGlyph type={param.type} />
                                  {param.type}
                                  {param.optional ? "?" : ""}
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
                      </section>
                    )}
                  </>
                ) : (
                  <p className="agent-tools-empty">{t("tools.detail.selectHint")}</p>
                )}
              </div>
            </div>
          ) : (
            <div className={`agent-tools-grid is-${viewMode}`}>
              {items.map((tool) => {
                const isOn = enabled[tool.id];
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
                      <h3 className="agent-tool-title">{t(tool.titleKey)}</h3>
                      <span className="agent-tool-call-stat">
                        {t("tools.callCount", {
                          n: String(usage?.tools[tool.id] ?? 0),
                        })}
                      </span>
                      <button
                        type="button"
                        role="switch"
                        className="tool-toggle"
                        aria-checked={isOn}
                        aria-label={t("agentTools.toggleAria", {
                          name: t(tool.titleKey),
                          state: isOn ? t("agentTools.on") : t("agentTools.off"),
                        })}
                        onClick={() => onToggle(tool.id)}
                      >
                        <span className="tool-toggle-thumb" />
                      </button>
                    </header>

                    <div className="tool-card-body agent-tool-detail">
                      <p className="agent-tool-desc">{t(tool.descKey)}</p>
                      {tool.params.length > 0 && (
                        <div className="agent-tool-params">
                          <span className="agent-tool-params-label">
                            {t("agentTools.params")}
                          </span>
                          <ul className="agent-tool-params-list">
                            {tool.params.map((param) => (
                              <li key={param.name} className="agent-tool-param">
                                <code className="agent-tool-param-name">
                                  {param.name}
                                </code>
                                <span className="agent-tool-param-type">
                                  {param.type}
                                  {param.optional ? "?" : ""}
                                </span>
                              </li>
                            ))}
                          </ul>
                        </div>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          )}
        </>
      )}

      {tab === "mcp" && (
        <>
          {servers.length === 0 ? (
            <div className="mcp-tools-empty">
              <div className="mcp-tools-empty-icon" aria-hidden>
                <Layers size={48} strokeWidth={1.5} aria-hidden />
              </div>
              <p className="mcp-tools-empty-title">{t("mcpTools.empty")}</p>
              <p className="mcp-tools-empty-hint">{t("mcpTools.emptyHint")}</p>
            </div>
          ) : filteredServers.length === 0 ? (
            <p className="agent-tools-empty">{t("agentTools.empty")}</p>
          ) : viewMode === "detail" ? (
            <div className="tools-detail">
              <div className="tools-detail-list" role="list">
                {filteredServers.map((server) => (
                  <button
                    key={server.id}
                    type="button"
                    role="listitem"
                    className={`tools-detail-item ${selectedDetailId === server.id ? "is-selected" : ""} ${server.enabled ? "" : "is-disabled"}`}
                    onClick={() => setSelectedDetailId(server.id)}
                  >
                    <span className="tools-detail-item-title">{server.name}</span>
                    <span className="tools-detail-item-meta">
                      {t(MCP_TYPE_LABEL[server.type])}
                    </span>
                  </button>
                ))}
              </div>
              <div className="tools-detail-panel">
                {selectedServer ? (
                  <>
                    <header className="tools-detail-head">
                      <div>
                        <h3 className="tools-detail-title">{selectedServer.name}</h3>
                        <button
                          type="button"
                          role="switch"
                          className="tool-toggle"
                          aria-checked={selectedServer.enabled}
                          onClick={() => toggleServer(selectedServer.id)}
                        >
                          <span className="tool-toggle-thumb" />
                        </button>
                      </div>
                      <button
                        type="button"
                        className="mcp-server-remove"
                        onClick={() => removeServer(selectedServer.id)}
                        aria-label={t("mcpTools.remove")}
                      >
                        <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                        {t("mcpTools.remove")}
                      </button>
                    </header>
                    {selectedServer.description && (
                      <section className="tools-detail-section">
                        <h4 className="tools-detail-label">
                          <FileText size={15} strokeWidth={2.25} aria-hidden />
                          {t("tools.detail.description")}
                        </h4>
                        <p className="tools-detail-body">
                          {selectedServer.description}
                        </p>
                      </section>
                    )}
                    <section className="tools-detail-meta-grid">
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("tools.detail.type")}
                        </span>
                        <span>{t(MCP_TYPE_LABEL[selectedServer.type])}</span>
                      </div>
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("tools.detail.endpoint")}
                        </span>
                        <code>{serverEndpoint(selectedServer)}</code>
                      </div>
                    </section>
                    <section className="tools-detail-section">
                      <div className="mcp-tool-list-head">
                        <h4 className="tools-detail-label">{t("mcpTools.tools")}</h4>
                        <button
                          type="button"
                          className="mcp-btn-ghost mcp-refresh-btn"
                          disabled={refreshing}
                          onClick={() => void refreshTools(selectedServer.id)}
                        >
                          <IconRefresh
                            width={14}
                            height={14}
                            className={refreshing ? "is-spin" : undefined}
                          />
                          {refreshing ? t("mcpTools.refreshing") : t("mcpTools.refresh")}
                        </button>
                      </div>
                      {(selectedServer.discovered.length > 0
                        ? selectedServer.discovered
                        : Object.keys(selectedServer.tools).map((name) => ({
                            name,
                            description: "",
                          }))
                      ).map((tool) => {
                        const on = selectedServer.tools[tool.name] ?? true;
                        return (
                          <div key={tool.name} className="mcp-tool-row">
                            <div className="mcp-tool-meta">
                              <code className="mcp-tool-name">{tool.name}</code>
                              {tool.description ? (
                                <span className="mcp-tool-desc">{tool.description}</span>
                              ) : null}
                            </div>
                            <button
                              type="button"
                              role="switch"
                              className="tool-toggle"
                              aria-checked={on}
                              disabled={!selectedServer.enabled}
                              onClick={() => toggleTool(selectedServer.id, tool.name)}
                            >
                              <span className="tool-toggle-thumb" />
                            </button>
                          </div>
                        );
                      })}
                    </section>
                  </>
                ) : (
                  <p className="agent-tools-empty">{t("tools.detail.selectHint")}</p>
                )}
              </div>
            </div>
          ) : (
            <div className={`mcp-server-grid is-${viewMode}`}>
              {filteredServers.map((s) => (
                <McpServerCard
                  key={s.id}
                  server={s}
                  onToggle={toggleServer}
                  onToggleTool={toggleTool}
                  onRefresh={(id) => void refreshTools(id)}
                  onRemove={removeServer}
                  refreshing={refreshing}
                />
              ))}
            </div>
          )}
        </>
      )}
      </AnimatedSwitch>

      {showAdd && (
        <McpAddDialog
          onAdd={(incoming) => { addServers(incoming); }}
          onClose={() => setShowAdd(false)}
        />
      )}
    </div>
  );
}
