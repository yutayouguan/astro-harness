/** 工具面板：内置工具开关、调用统计与 MCP。 */
import { useEffect, useMemo, useRef, useState, type CSSProperties, type SVGProps } from "react";
import { createPortal } from "react-dom";
import {
  AlignLeft,
  Braces,
  Clock3,
  Columns2,
  FileJson2,
  FileText,
  FormInput,
  FunctionSquare,
  Globe,
  Hash,
  Heading,
  KeyRound,
  LayoutGrid,
  Link2,
  List,
  CirclePlus,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Tag,
  Terminal,
  Timer,
  Trash2,
  Type,
  Wrench,
  X,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useAgentTools } from "../../hooks/providers/useAgentTools";
import {
  parseMcpJson,
  LegacySseTransportError,
  DEFAULT_MCP_STARTUP_TIMEOUT_SECS,
  DEFAULT_MCP_TOOL_TIMEOUT_SECS,
  MAX_MCP_STARTUP_TIMEOUT_SECS,
  MAX_MCP_TOOL_TIMEOUT_SECS,
  isMcpToolEnabled,
  useMcpTools,
  type McpRuntimeState,
  type McpRuntimeStatus,
  type McpServer,
  type McpToolApprovalMode,
  type McpTransportType,
} from "../../hooks/providers/useMcpTools";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import McpIcon from "../icons/McpIcon";
import AnimatedSwitch from "../ui/AnimatedSwitch";
import ExpandableSearch from "../ui/ExpandableSearch";
import LucideByName from "../icons/LucideByName";
import { IconRefresh } from "../icons/NavIcons";
import { SelectMenu } from "../ui/SelectMenu";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";
import SecurityAuditSection from "./SecurityAuditSection";

/** 工具面板 Tab：内置 / MCP / 审批 */
type ToolTab = "builtin" | "mcp" | "approvals";

/** 危险命令审批设置（Tauri camelCase） */
type ApprovalSettings = { mode: string; commandAllowlist: string[] };

const APPROVAL_MODES = ["smart", "manual", "off"] as const;
const MCP_APPROVAL_MODES: McpToolApprovalMode[] = ["auto", "prompt", "writes", "approve"];

/** 危险命令审批设置区（全局，非按 Agent） */
function ApprovalsSection({ active }: { active: boolean }) {
  const { t } = useI18n();
  const [settings, setSettings] = useState<ApprovalSettings | null>(null);
  const [newEntry, setNewEntry] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!active || !isTauri()) return;
    void (async () => {
      try {
        setSettings(await invoke<ApprovalSettings>("get_approval_settings"));
      } catch {
        /* ignore */
      }
    })();
  }, [active]);

  const setMode = async (mode: string) => {
    try {
      setSettings(await invoke<ApprovalSettings>("set_approval_mode", { mode }));
    } catch {
      /* ignore */
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
    } catch {
      /* ignore */
    } finally {
      setBusy(false);
    }
  };

  const removeEntry = async (entry: string) => {
    try {
      setSettings(
        await invoke<ApprovalSettings>("remove_command_allowlist", { entry }),
      );
    } catch {
      /* ignore */
    }
  };

  if (!settings) {
    return <p className="agent-tools-empty">{t("approvals.loading")}</p>;
  }

  const modeOptions = APPROVAL_MODES.map((m) => ({
    value: m,
    label: t(`approvals.mode.${m}` as MessageKey),
  }));

  return (
    <div className="approvals-section">
      <section className="tools-detail-section">
        <h4 className="tools-detail-label">
          <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
          {t("approvals.mode.label")}
        </h4>
        <SelectMenu
          className="approvals-mode-select"
          value={settings.mode}
          onChange={(v) => void setMode(v)}
          options={modeOptions}
          aria-label={t("approvals.mode.label")}
        />
        <p className="tools-detail-body">
          {t(`approvals.mode.hint.${settings.mode}` as MessageKey)}
        </p>
      </section>

      <section className="tools-detail-section">
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

const MCP_TYPES: McpTransportType[] = ["stdio", "streamableHttp"];

const MCP_TYPE_LABEL: Record<McpTransportType, MessageKey> = {
  stdio: "mcpTools.type.stdio",
  streamableHttp: "mcpTools.type.streamableHttp",
};

const MCP_STATUS_LABEL: Record<McpRuntimeState, MessageKey> = {
  configured: "mcpTools.status.configured",
  disabled: "mcpTools.status.disabled",
  connecting: "mcpTools.status.connecting",
  connected: "mcpTools.status.connected",
  disconnected: "mcpTools.status.disconnected",
  backoff: "mcpTools.status.backoff",
  "auth-required": "mcpTools.status.authRequired",
  error: "mcpTools.status.error",
  unknown: "mcpTools.status.unknown",
};

const MCP_TYPE_ICON: Record<McpTransportType, typeof Terminal> = {
  stdio: Terminal,
  streamableHttp: Globe,
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

/** 新增 MCP Server 右侧抽屉 */
function McpAddDialog({
  onAdd,
  onClose,
  toneStyle,
}: {
  onAdd: (servers: McpServer[]) => void;
  onClose: () => void;
  toneStyle?: CSSProperties;
}) {
  const { t } = useI18n();
  const [addTab, setAddTab] = useState<AddTab>("json");
  const [jsonText, setJsonText] = useState("");
  const [jsonError, setJsonError] = useState("");

  const [formName, setFormName] = useState("");
  const [formDesc, setFormDesc] = useState("");
  const [formType, setFormType] = useState<McpTransportType>("stdio");
  const [formCommand, setFormCommand] = useState("");
  const [formCwd, setFormCwd] = useState("");
  const [formArgs, setFormArgs] = useState("");
  const [formEnv, setFormEnv] = useState("");
  const [formEnvVars, setFormEnvVars] = useState("");
  const [formUrl, setFormUrl] = useState("");
  const [formHeaders, setFormHeaders] = useState("");
  const [formBearerTokenEnvVar, setFormBearerTokenEnvVar] = useState("");
  const [formEnvHeaders, setFormEnvHeaders] = useState("");
  const [formStartupTimeout, setFormStartupTimeout] = useState(
    String(DEFAULT_MCP_STARTUP_TIMEOUT_SECS),
  );
  const [formToolTimeout, setFormToolTimeout] = useState(
    String(DEFAULT_MCP_TOOL_TIMEOUT_SECS),
  );
  const [formRequired, setFormRequired] = useState(false);
  const [formError, setFormError] = useState("");

  const backdropRef = useRef<HTMLDivElement>(null);
  const isStdio = formType === "stdio";
  const TypeIcon = MCP_TYPE_ICON[formType];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onClose]);

  function handleBackdrop(e: React.MouseEvent) {
    if (e.target === backdropRef.current) onClose();
  }

  function handleJsonImport() {
    setJsonError("");
    try {
      const servers = parseMcpJson(jsonText.trim());
      onAdd(servers);
      onClose();
    } catch (error) {
      setJsonError(
        error instanceof LegacySseTransportError
          ? t("mcpTools.legacySseError")
          : t("mcpTools.jsonError"),
      );
    }
  }

  function handleFormAdd() {
    setFormError("");
    if (!formName.trim()) {
      setFormError(t("mcpTools.formNameRequired"));
      return;
    }
    const startupTimeoutSecs = Number(formStartupTimeout);
    const toolTimeoutSecs = Number(formToolTimeout);
    if (
      !Number.isInteger(startupTimeoutSecs) ||
      startupTimeoutSecs < 1 ||
      startupTimeoutSecs > MAX_MCP_STARTUP_TIMEOUT_SECS ||
      !Number.isInteger(toolTimeoutSecs) ||
      toolTimeoutSecs < 1 ||
      toolTimeoutSecs > MAX_MCP_TOOL_TIMEOUT_SECS
    ) {
      setFormError(t("mcpTools.formTimeoutRangeError"));
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
    const envVars = formEnvVars
      .split("\n")
      .map((s) => s.trim())
      .filter(Boolean);
    const envHttpHeaders = parseEnvOrHeaders(formEnvHeaders);
    const bearerTokenEnvVar = formBearerTokenEnvVar.trim();
    const referencedEnvVars = isStdio
      ? envVars
      : [
          ...Object.values(envHttpHeaders),
          ...(bearerTokenEnvVar ? [bearerTokenEnvVar] : []),
        ];
    if (referencedEnvVars.some((name) => !/^[A-Za-z_][A-Za-z0-9_]*$/.test(name))) {
      setFormError(t("mcpTools.formEnvVarError"));
      return;
    }

    onAdd([
      {
        id: `mcp_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
        name: formName.trim(),
        description: formDesc.trim(),
        type: formType,
        command: isStdio ? formCommand.trim() : "",
        args: isStdio ? args : [],
        env: isStdio ? parseEnvOrHeaders(formEnv) : {},
        envVars: isStdio ? envVars : [],
        url: isStdio ? "" : formUrl.trim(),
        headers: isStdio ? {} : parseEnvOrHeaders(formHeaders),
        bearerTokenEnvVar: !isStdio && bearerTokenEnvVar ? bearerTokenEnvVar : undefined,
        envHttpHeaders: isStdio ? {} : envHttpHeaders,
        enabled: true,
        required: formRequired,
        cwd: isStdio && formCwd.trim() ? formCwd.trim() : undefined,
        startupTimeoutSecs,
        toolTimeoutSecs,
        enabledTools: undefined,
        disabledTools: [],
        defaultToolsApprovalMode: "auto",
        tools: {},
        toolApprovalModes: {},
        discovered: [],
      },
    ]);
    onClose();
  }

  return createPortal(
    <div
      className="mcp-add-drawer-backdrop"
      ref={backdropRef}
      style={toneStyle}
      onClick={handleBackdrop}
    >
      <aside
        className="mcp-add-drawer"
        role="dialog"
        aria-modal
        aria-labelledby="mcp-add-drawer-title"
      >
        <header className="mcp-add-drawer-head">
          <h2 id="mcp-add-drawer-title" className="mcp-add-drawer-title">
            <span className="mcp-add-drawer-title-icon" aria-hidden>
              <McpIcon size={18} />
            </span>
            {t("mcpTools.addTitle")}
          </h2>
          <button
            type="button"
            className="mcp-add-drawer-close"
            onClick={onClose}
            aria-label={t("mcpTools.cancel")}
          >
            <X size={16} strokeWidth={2.5} aria-hidden />
          </button>
        </header>

        <div className="mcp-add-drawer-tabs" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={addTab === "json"}
            className={`mcp-add-drawer-tab ${addTab === "json" ? "active" : ""}`}
            onClick={() => {
              setAddTab("json");
              setJsonError("");
            }}
          >
            <Braces size={14} strokeWidth={2.25} aria-hidden />
            <span>{t("mcpTools.tabJson")}</span>
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={addTab === "form"}
            className={`mcp-add-drawer-tab ${addTab === "form" ? "active" : ""}`}
            onClick={() => {
              setAddTab("form");
              setFormError("");
            }}
          >
            <FormInput size={14} strokeWidth={2.25} aria-hidden />
            <span>{t("mcpTools.tabForm")}</span>
          </button>
        </div>

        <div className="mcp-add-drawer-scroll">
          <AnimatedSwitch switchKey={addTab} variant="fade">
            {addTab === "json" && (
              <div className="mcp-add-drawer-body">
                <label className="mcp-field mcp-field--full">
                  <span className="mcp-field-label">
                    <Braces size={13} strokeWidth={2.2} aria-hidden />
                    {t("mcpTools.tabJson")}
                  </span>
                  <textarea
                    className="mcp-add-drawer-textarea"
                    value={jsonText}
                    onChange={(e) => setJsonText(e.target.value)}
                    placeholder={t("mcpTools.jsonPlaceholder")}
                    spellCheck={false}
                    rows={10}
                  />
                </label>
                {jsonError ? (
                  <p className="mcp-add-drawer-error">{jsonError}</p>
                ) : null}
              </div>
            )}

            {addTab === "form" && (
              <div className="mcp-add-drawer-body">
                <div className="mcp-form-grid">
                  <label className="mcp-field mcp-field--full">
                    <span className="mcp-field-label">
                      <Tag size={13} strokeWidth={2.2} aria-hidden />
                      {t("mcpTools.formName")}
                    </span>
                    <input
                      type="text"
                      value={formName}
                      onChange={(e) => setFormName(e.target.value)}
                      placeholder={t("mcpTools.formNamePlaceholder")}
                    />
                  </label>
                  <label className="mcp-field mcp-field--full">
                    <span className="mcp-field-label">
                      <AlignLeft size={13} strokeWidth={2.2} aria-hidden />
                      {t("mcpTools.formDesc")}
                    </span>
                    <input
                      type="text"
                      value={formDesc}
                      onChange={(e) => setFormDesc(e.target.value)}
                      placeholder={t("mcpTools.formDescPlaceholder")}
                    />
                  </label>
                  <div className="mcp-field mcp-field--full">
                    <span className="mcp-field-label">
                      <TypeIcon size={13} strokeWidth={2.2} aria-hidden />
                      {t("mcpTools.formType")}
                    </span>
                    <SelectMenu
                      className="mcp-type-select"
                      value={formType}
                      onChange={(v) => setFormType(v as McpTransportType)}
                      aria-label={t("mcpTools.formType")}
                      options={MCP_TYPES.map((type) => {
                        const Icon = MCP_TYPE_ICON[type];
                        return {
                          value: type,
                          label: t(MCP_TYPE_LABEL[type]),
                          icon: <Icon size={13} strokeWidth={2.2} aria-hidden />,
                        };
                      })}
                    />
                  </div>
                  <label className="mcp-field">
                    <span className="mcp-field-label">
                      <Clock3 size={13} strokeWidth={2.2} aria-hidden />
                      {t("mcpTools.formStartupTimeout")}
                    </span>
                    <input
                      type="number"
                      min={1}
                      max={MAX_MCP_STARTUP_TIMEOUT_SECS}
                      step={1}
                      value={formStartupTimeout}
                      onChange={(e) => setFormStartupTimeout(e.target.value)}
                    />
                  </label>
                  <label className="mcp-field">
                    <span className="mcp-field-label">
                      <Timer size={13} strokeWidth={2.2} aria-hidden />
                      {t("mcpTools.formToolTimeout")}
                    </span>
                    <input
                      type="number"
                      min={1}
                      max={MAX_MCP_TOOL_TIMEOUT_SECS}
                      step={1}
                      value={formToolTimeout}
                      onChange={(e) => setFormToolTimeout(e.target.value)}
                    />
                  </label>
                  <div className="mcp-field">
                    <span className="mcp-field-label">
                      {t("mcpTools.formRequired")}
                    </span>
                    <button
                      type="button"
                      role="switch"
                      className="tool-toggle"
                      aria-checked={formRequired}
                      onClick={() => setFormRequired((required) => !required)}
                    >
                      <span className="tool-toggle-thumb" />
                    </button>
                  </div>

                  {isStdio ? (
                    <>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <Terminal size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formCommand")}
                        </span>
                        <input
                          type="text"
                          value={formCommand}
                          onChange={(e) => setFormCommand(e.target.value)}
                          placeholder={t("mcpTools.formCommandPlaceholder")}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          {t("mcpTools.formCwd")}
                        </span>
                        <input
                          type="text"
                          value={formCwd}
                          onChange={(e) => setFormCwd(e.target.value)}
                          placeholder={t("mcpTools.formCwdPlaceholder")}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <List size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formArgs")}
                        </span>
                        <textarea
                          className="mcp-field-textarea"
                          value={formArgs}
                          onChange={(e) => setFormArgs(e.target.value)}
                          placeholder={t("mcpTools.formArgsPlaceholder")}
                          rows={3}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formEnvVars")}
                        </span>
                        <textarea
                          className="mcp-field-textarea"
                          value={formEnvVars}
                          onChange={(e) => setFormEnvVars(e.target.value)}
                          placeholder={t("mcpTools.formEnvVarsPlaceholder")}
                          rows={3}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formEnv")}
                        </span>
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
                        <span className="mcp-field-label">
                          <Link2 size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formUrl")}
                        </span>
                        <input
                          type="url"
                          value={formUrl}
                          onChange={(e) => setFormUrl(e.target.value)}
                          placeholder={t("mcpTools.formUrlPlaceholder")}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <Heading size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formHeaders")}
                        </span>
                        <textarea
                          className="mcp-field-textarea"
                          value={formHeaders}
                          onChange={(e) => setFormHeaders(e.target.value)}
                          placeholder={t("mcpTools.formHeadersPlaceholder")}
                          rows={3}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formBearerTokenEnvVar")}
                        </span>
                        <input
                          type="text"
                          value={formBearerTokenEnvVar}
                          onChange={(e) => setFormBearerTokenEnvVar(e.target.value)}
                          placeholder={t("mcpTools.formBearerTokenEnvVarPlaceholder")}
                          autoCapitalize="off"
                          autoCorrect="off"
                          spellCheck={false}
                        />
                      </label>
                      <label className="mcp-field mcp-field--full">
                        <span className="mcp-field-label">
                          <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                          {t("mcpTools.formEnvHeaders")}
                        </span>
                        <textarea
                          className="mcp-field-textarea"
                          value={formEnvHeaders}
                          onChange={(e) => setFormEnvHeaders(e.target.value)}
                          placeholder={t("mcpTools.formEnvHeadersPlaceholder")}
                          rows={3}
                        />
                      </label>
                    </>
                  )}
                </div>
                {formError ? (
                  <p className="mcp-add-drawer-error">{formError}</p>
                ) : null}
              </div>
            )}
          </AnimatedSwitch>
        </div>

        <footer className="mcp-add-drawer-foot">
          <button type="button" className="mcp-btn-ghost" onClick={onClose}>
            <X size={14} strokeWidth={2.3} aria-hidden />
            {t("mcpTools.cancel")}
          </button>
          {addTab === "json" ? (
            <button
              type="button"
              className="mcp-btn-primary"
              onClick={handleJsonImport}
              disabled={!jsonText.trim()}
            >
              <Plus size={14} strokeWidth={2.3} aria-hidden />
              {t("mcpTools.jsonImport")}
            </button>
          ) : (
            <button
              type="button"
              className="mcp-btn-primary"
              onClick={handleFormAdd}
            >
              <Plus size={14} strokeWidth={2.3} aria-hidden />
              {t("mcpTools.formAdd")}
            </button>
          )}
        </footer>
      </aside>
    </div>,
    document.body,
  );
}

/** MCP 卡片上展示的命令行或 URL */
function serverEndpoint(server: McpServer): string {
  if (server.type === "stdio") {
    return [server.command, ...server.args].filter(Boolean).join(" ");
  }
  return server.url || "—";
}

function McpStatusBadge({ status }: { status?: McpRuntimeStatus }) {
  const { t } = useI18n();
  if (!status) return null;
  return (
    <span className="mcp-runtime-status" data-status={status.status}>
      <span className="mcp-runtime-status-dot" aria-hidden />
      {t(MCP_STATUS_LABEL[status.status])}
    </span>
  );
}

function McpRetryNote({ status }: { status?: McpRuntimeStatus }) {
  const { t } = useI18n();
  if (!status?.retryable || status.retryAttempt === 0) return null;
  const time = status.nextRetryAtUnixMs
    ? new Date(status.nextRetryAtUnixMs).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : "—";
  return (
    <p className="mcp-retry-note">
      {t("mcpTools.retryScheduled", {
        attempt: String(status.retryAttempt),
        time,
      })}
    </p>
  );
}

/** 单个 MCP Server 卡片（开关、工具列表、刷新） */
function McpServerCard({
  server,
  onToggle,
  onToggleTool,
  onSetServerApprovalMode,
  onSetToolApprovalMode,
  onRefresh,
  onRemove,
  onReconnect,
  onAuthenticate,
  onLogout,
  refreshing,
  reconnecting,
  authenticating,
  runtimeStatus,
}: {
  server: McpServer;
  onToggle: (id: string) => void;
  onToggleTool: (serverId: string, toolName: string) => void;
  onSetServerApprovalMode: (serverId: string, mode: McpToolApprovalMode) => void;
  onSetToolApprovalMode: (
    serverId: string,
    toolName: string,
    mode?: McpToolApprovalMode,
  ) => void;
  onRefresh: (serverId: string) => void;
  onRemove: (id: string) => void;
  onReconnect: (id: string) => void;
  onAuthenticate: (id: string) => void;
  onLogout: (id: string) => void;
  refreshing?: boolean;
  reconnecting?: boolean;
  authenticating?: boolean;
  runtimeStatus?: McpRuntimeStatus;
}) {
  const { t } = useI18n();
  const headerKeys = Object.keys(server.headers ?? {});
  const envKeys = Object.keys(server.env ?? {});
  const forwardedEnvKeys = server.envVars ?? [];
  const envHeaderEntries = Object.entries(server.envHttpHeaders ?? {});
  const approvalOptions = MCP_APPROVAL_MODES.map((mode) => ({
    value: mode,
    label: t(`mcpTools.approval.${mode}` as MessageKey),
  }));
  const toolApprovalOptions = [
    { value: "inherit", label: t("mcpTools.approval.inherit") },
    ...approvalOptions,
  ];
  const toolRows =
    server.discovered.length > 0
      ? server.discovered
      : Object.keys(server.tools).map((name) => ({
          name,
          description: "",
          readOnlyHint: undefined,
          destructiveHint: undefined,
          openWorldHint: undefined,
        }));
  return (
    <article
      className={`mcp-server-card ${server.enabled ? "is-on" : "is-off"}`}
    >
      <header className="mcp-server-head">
        <div className="mcp-server-icon" aria-hidden>
          <McpIcon size={22} />
        </div>
        <div className="mcp-server-meta">
          <div className="mcp-server-title-row">
            <span className="mcp-server-name">{server.name}</span>
            <span className="mcp-server-type">{t(MCP_TYPE_LABEL[server.type])}</span>
          </div>
          <code className="mcp-server-cmd">{serverEndpoint(server)}</code>
          <McpStatusBadge status={runtimeStatus} />
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
      {runtimeStatus?.error ? (
        <p className="mcp-runtime-error" role="alert">
          <ShieldAlert size={13} strokeWidth={2.2} aria-hidden />
          <span>{runtimeStatus.error}</span>
        </p>
      ) : null}
      <McpRetryNote status={runtimeStatus} />
      <div className="mcp-server-env">
        <span className="mcp-server-env-key">
          {t("mcpTools.startupTimeout")}: {server.startupTimeoutSecs}s
        </span>
        <span className="mcp-server-env-key">
          {t("mcpTools.toolTimeout")}: {server.toolTimeoutSecs}s
        </span>
        {server.required ? (
          <span className="mcp-server-env-key">{t("mcpTools.required")}</span>
        ) : null}
        {server.cwd ? (
          <span className="mcp-server-env-key">
            {t("mcpTools.cwd")}: {server.cwd}
          </span>
        ) : null}
        {envKeys.map((k) => (
          <span key={`env-${k}`} className="mcp-server-env-key">{k}</span>
        ))}
        {forwardedEnvKeys.map((k) => (
          <span key={`env-ref-${k}`} className="mcp-server-env-key">{k} ← env</span>
        ))}
        {headerKeys.map((k) => (
          <span key={`hdr-${k}`} className="mcp-server-env-key">{k}</span>
        ))}
        {server.bearerTokenEnvVar ? (
          <span className="mcp-server-env-key">
            Authorization ← {server.bearerTokenEnvVar}
          </span>
        ) : null}
        {envHeaderEntries.map(([header, envVar]) => (
          <span key={`env-hdr-${header}`} className="mcp-server-env-key">
            {header} ← {envVar}
          </span>
        ))}
      </div>
      <div className="mcp-approval-setting">
        <div>
          <span className="mcp-tool-list-label">{t("mcpTools.approval.default")}</span>
          <span className="mcp-approval-hint">
            {t(`mcpTools.approval.hint.${server.defaultToolsApprovalMode}` as MessageKey)}
          </span>
        </div>
        <SelectMenu
          size="sm"
          value={server.defaultToolsApprovalMode}
          options={approvalOptions}
          onChange={(value) =>
            onSetServerApprovalMode(server.id, value as McpToolApprovalMode)
          }
          aria-label={t("mcpTools.approval.default")}
        />
      </div>
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
              const on = isMcpToolEnabled(server, tool.name);
              return (
                <li key={tool.name} className="mcp-tool-row">
                  <div className="mcp-tool-meta">
                    <code className="mcp-tool-name">{tool.name}</code>
                    {tool.description ? (
                      <span className="mcp-tool-desc">{tool.description}</span>
                    ) : null}
                    <span className="mcp-tool-hints">
                      {tool.readOnlyHint === true ? (
                        <span>{t("mcpTools.annotation.readOnly")}</span>
                      ) : null}
                      {tool.destructiveHint === true ? (
                        <span>{t("mcpTools.annotation.destructive")}</span>
                      ) : null}
                      {tool.openWorldHint === true ? (
                        <span>{t("mcpTools.annotation.openWorld")}</span>
                      ) : null}
                    </span>
                  </div>
                  <div className="mcp-tool-controls">
                    <SelectMenu
                      size="sm"
                      value={server.toolApprovalModes[tool.name] ?? "inherit"}
                      options={toolApprovalOptions}
                      disabled={!server.enabled}
                      onChange={(value) =>
                        onSetToolApprovalMode(
                          server.id,
                          tool.name,
                          value === "inherit" ? undefined : (value as McpToolApprovalMode),
                        )
                      }
                      aria-label={t("mcpTools.approval.tool", { name: tool.name })}
                    />
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
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      <div className="mcp-server-actions">
        {runtimeStatus?.status === "auth-required" ? (
          <button
            type="button"
            className="mcp-btn-ghost"
            disabled={authenticating}
            onClick={() => onAuthenticate(server.id)}
          >
            <KeyRound size={13} strokeWidth={2.2} aria-hidden />
            {authenticating ? t("mcpTools.authenticating") : t("mcpTools.authenticate")}
          </button>
        ) : null}
        {runtimeStatus?.authenticated ? (
          <button
            type="button"
            className="mcp-btn-ghost"
            disabled={authenticating}
            onClick={() => onLogout(server.id)}
          >
            <KeyRound size={13} strokeWidth={2.2} aria-hidden />
            {authenticating ? t("mcpTools.loggingOut") : t("mcpTools.logout")}
          </button>
        ) : null}
        <button
          type="button"
          className="mcp-btn-ghost mcp-reconnect-btn"
          disabled={reconnecting}
          onClick={() => onReconnect(server.id)}
        >
          <IconRefresh
            width={13}
            height={13}
            className={reconnecting ? "is-spin" : undefined}
          />
          {reconnecting ? t("mcpTools.reconnecting") : t("mcpTools.reconnect")}
        </button>
        <button
          type="button"
          className="mcp-server-remove"
          onClick={() => onRemove(server.id)}
          aria-label={t("mcpTools.remove")}
        >
          <Trash2 size={13} strokeWidth={2.25} aria-hidden />
          {t("mcpTools.remove")}
        </button>
      </div>
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
  const pageRef = useRef<HTMLDivElement>(null);
  const [showAdd, setShowAdd] = useState(false);
  const [search, setSearch] = useState("");
  const { activeAgentId: agentId } = useActiveAgent();
  const [viewMode, setViewMode] = useState<ToolsView>(() => readToolsView());
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const [selectedFnName, setSelectedFnName] = useState<string | null>(null);
  const [usage, setUsage] = useState<AgentUsageSummary | null>(null);
  const { enabled, toggle: onToggle, tools: agentTools } = useAgentTools(agentId);
  const {
    servers,
    addServers,
    toggleServer,
    toggleTool,
    setServerApprovalMode,
    setToolApprovalMode,
    removeServer,
    refreshTools,
    refreshing,
    runtimeStatuses,
    runtimeStatusError,
    reconnectServer,
    reconnectingServerIds,
    authenticateServer,
    logoutServer,
    authenticatingServerIds,
  } = useMcpTools(agentId, active && tab === "mcp");
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
  const selectedRuntimeStatus = selectedServer
    ? runtimeStatuses[selectedServer.id]
    : undefined;

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
    <div className="agent-tools-page" ref={pageRef}>
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
            aria-selected={tab === "mcp"}
            className={`tool-main-tab ${tab === "mcp" ? "active" : ""}`}
            onClick={() => setTab("mcp")}
          >
            <McpIcon size={15} />
            {t("tools.tab.mcp")}
            {servers.length > 0 && (
              <span className="tool-main-tab-count">{servers.length}</span>
            )}
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
            {tab === "mcp" && (
              <button
                type="button"
                className="mcp-add-btn"
                onClick={() => setShowAdd(true)}
                title={t("mcpTools.add")}
                aria-label={t("mcpTools.add")}
              >
                <CirclePlus size={17} />
              </button>
            )}
          </div>
        )}
      </div>

      <div className="agent-tools-body">
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
                          <div className="tools-detail-ids">
                            <span className="tools-detail-id-chip" title={t("tools.detail.toolset")}>
                              <span className="tools-detail-id-label">
                                {t("tools.detail.toolset")}
                              </span>
                              <code>{selectedTool.id}</code>
                            </span>
                            <span className="tools-detail-id-chip" title={t("tools.detail.apiName")}>
                              <span className="tools-detail-id-label">
                                {t("tools.detail.apiName")}
                              </span>
                              <code>
                                {activeFn?.name ??
                                  selectedTool.tools?.[0] ??
                                  selectedTool.id}
                              </code>
                            </span>
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
                    <section className="tools-detail-section">
                      <h4 className="tools-detail-label">
                        <FileText size={15} strokeWidth={2.25} aria-hidden />
                        {t("tools.detail.description")}
                      </h4>
                      <p className="tools-detail-body">{t(selectedTool.descKey)}</p>
                      {detailApiDesc ? (
                        <div className="tools-detail-api-block">
                          <h4 className="tools-detail-label">
                            <FileJson2 size={15} strokeWidth={2.25} aria-hidden />
                            {t("tools.detail.apiSchema")}
                          </h4>
                          <p className="tools-detail-api-desc">{detailApiDesc}</p>
                        </div>
                      ) : null}
                    </section>
                    {(selectedTool.functions?.length ?? 0) > 0 && (
                      <section className="tools-detail-section">
                        <h4 className="tools-detail-label">
                          <FunctionSquare size={15} strokeWidth={2.25} aria-hidden />
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
                                activeFn?.name === fn.name ? "is-active" : ""
                              }`}
                              onClick={() => setSelectedFnName(fn.name)}
                              title={fn.description || fn.name}
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
                        <Braces size={15} strokeWidth={2.25} aria-hidden />
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
                            <li key={param.name} className="agent-tool-param is-detail">
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
          {runtimeStatusError ? (
            <p className="mcp-runtime-error mcp-runtime-error--global" role="alert">
              <ShieldAlert size={13} strokeWidth={2.2} aria-hidden />
              <span>{t("mcpTools.statusUnavailable")}: {runtimeStatusError}</span>
            </p>
          ) : null}
          {servers.length === 0 ? (
            <div className="mcp-tools-empty">
              <div className="mcp-tools-empty-icon" aria-hidden>
                <McpIcon size={48} />
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
                      {runtimeStatuses[server.id]
                        ? t(MCP_STATUS_LABEL[runtimeStatuses[server.id]!.status])
                        : t(MCP_TYPE_LABEL[server.type])}
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
                      <div className="mcp-server-actions">
                        {selectedRuntimeStatus?.status === "auth-required" ? (
                          <button
                            type="button"
                            className="mcp-btn-ghost"
                            disabled={authenticatingServerIds.has(selectedServer.id)}
                            onClick={() => void authenticateServer(selectedServer.id)}
                          >
                            <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                            {authenticatingServerIds.has(selectedServer.id)
                              ? t("mcpTools.authenticating")
                              : t("mcpTools.authenticate")}
                          </button>
                        ) : null}
                        {selectedRuntimeStatus?.authenticated ? (
                          <button
                            type="button"
                            className="mcp-btn-ghost"
                            disabled={authenticatingServerIds.has(selectedServer.id)}
                            onClick={() => void logoutServer(selectedServer.id)}
                          >
                            <KeyRound size={13} strokeWidth={2.2} aria-hidden />
                            {authenticatingServerIds.has(selectedServer.id)
                              ? t("mcpTools.loggingOut")
                              : t("mcpTools.logout")}
                          </button>
                        ) : null}
                        <button
                          type="button"
                          className="mcp-btn-ghost mcp-reconnect-btn"
                          disabled={reconnectingServerIds.has(selectedServer.id)}
                          onClick={() => void reconnectServer(selectedServer.id)}
                        >
                          <IconRefresh
                            width={13}
                            height={13}
                            className={
                              reconnectingServerIds.has(selectedServer.id)
                                ? "is-spin"
                                : undefined
                            }
                          />
                          {reconnectingServerIds.has(selectedServer.id)
                            ? t("mcpTools.reconnecting")
                            : t("mcpTools.reconnect")}
                        </button>
                        <button
                          type="button"
                          className="mcp-server-remove"
                          onClick={() => removeServer(selectedServer.id)}
                          aria-label={t("mcpTools.remove")}
                        >
                          <Trash2 size={13} strokeWidth={2.25} aria-hidden />
                          {t("mcpTools.remove")}
                        </button>
                      </div>
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
                    {selectedRuntimeStatus?.error ? (
                      <p className="mcp-runtime-error" role="alert">
                        <ShieldAlert size={13} strokeWidth={2.2} aria-hidden />
                        <span>{selectedRuntimeStatus.error}</span>
                      </p>
                    ) : null}
                    <McpRetryNote status={selectedRuntimeStatus} />
                    <section className="tools-detail-meta-grid">
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("mcpTools.runtimeStatus")}
                        </span>
                        {selectedRuntimeStatus ? (
                          <McpStatusBadge status={selectedRuntimeStatus} />
                        ) : (
                          <span>{t("mcpTools.status.loading")}</span>
                        )}
                      </div>
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
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("mcpTools.startupTimeout")}
                        </span>
                        <span>{selectedServer.startupTimeoutSecs}s</span>
                      </div>
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("mcpTools.toolTimeout")}
                        </span>
                        <span>{selectedServer.toolTimeoutSecs}s</span>
                      </div>
                      <div className="tools-detail-meta-item">
                        <span className="tools-detail-label">
                          {t("mcpTools.required")}
                        </span>
                        <span>
                          {selectedServer.required
                            ? t("mcpTools.enabled")
                            : t("mcpTools.disabled")}
                        </span>
                      </div>
                      {selectedServer.cwd ? (
                        <div className="tools-detail-meta-item">
                          <span className="tools-detail-label">{t("mcpTools.cwd")}</span>
                          <code>{selectedServer.cwd}</code>
                        </div>
                      ) : null}
                    </section>
                    <section className="tools-detail-section">
                      <h4 className="tools-detail-label">
                        <ShieldCheck size={15} strokeWidth={2.25} aria-hidden />
                        {t("mcpTools.approval.default")}
                      </h4>
                      <SelectMenu
                        size="sm"
                        value={selectedServer.defaultToolsApprovalMode}
                        options={MCP_APPROVAL_MODES.map((mode) => ({
                          value: mode,
                          label: t(`mcpTools.approval.${mode}` as MessageKey),
                        }))}
                        onChange={(value) =>
                          setServerApprovalMode(
                            selectedServer.id,
                            value as McpToolApprovalMode,
                          )
                        }
                        aria-label={t("mcpTools.approval.default")}
                      />
                      <p className="tools-detail-body">
                        {t(
                          `mcpTools.approval.hint.${selectedServer.defaultToolsApprovalMode}` as MessageKey,
                        )}
                      </p>
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
                            readOnlyHint: undefined,
                            destructiveHint: undefined,
                            openWorldHint: undefined,
                          }))
                      ).map((tool) => {
                        const on = isMcpToolEnabled(selectedServer, tool.name);
                        return (
                          <div key={tool.name} className="mcp-tool-row">
                            <div className="mcp-tool-meta">
                              <code className="mcp-tool-name">{tool.name}</code>
                              {tool.description ? (
                                <span className="mcp-tool-desc">{tool.description}</span>
                              ) : null}
                              <span className="mcp-tool-hints">
                                {tool.readOnlyHint === true ? (
                                  <span>{t("mcpTools.annotation.readOnly")}</span>
                                ) : null}
                                {tool.destructiveHint === true ? (
                                  <span>{t("mcpTools.annotation.destructive")}</span>
                                ) : null}
                                {tool.openWorldHint === true ? (
                                  <span>{t("mcpTools.annotation.openWorld")}</span>
                                ) : null}
                              </span>
                            </div>
                            <div className="mcp-tool-controls">
                              <SelectMenu
                                size="sm"
                                value={selectedServer.toolApprovalModes[tool.name] ?? "inherit"}
                                options={[
                                  { value: "inherit", label: t("mcpTools.approval.inherit") },
                                  ...MCP_APPROVAL_MODES.map((mode) => ({
                                    value: mode,
                                    label: t(`mcpTools.approval.${mode}` as MessageKey),
                                  })),
                                ]}
                                disabled={!selectedServer.enabled}
                                onChange={(value) =>
                                  setToolApprovalMode(
                                    selectedServer.id,
                                    tool.name,
                                    value === "inherit"
                                      ? undefined
                                      : (value as McpToolApprovalMode),
                                  )
                                }
                                aria-label={t("mcpTools.approval.tool", { name: tool.name })}
                              />
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
                  onSetServerApprovalMode={setServerApprovalMode}
                  onSetToolApprovalMode={setToolApprovalMode}
                  onRefresh={(id) => void refreshTools(id)}
                  onRemove={removeServer}
                  onReconnect={(id) => void reconnectServer(id)}
                  onAuthenticate={(id) => void authenticateServer(id)}
                  onLogout={(id) => void logoutServer(id)}
                  refreshing={refreshing}
                  reconnecting={reconnectingServerIds.has(s.id)}
                  authenticating={authenticatingServerIds.has(s.id)}
                  runtimeStatus={runtimeStatuses[s.id]}
                />
              ))}
            </div>
          )}
        </>
      )}

      {tab === "approvals" && <ApprovalsSection active={active} />}
      </AnimatedSwitch>
      </div>

      {showAdd && (
        <McpAddDialog
          onAdd={(incoming) => { addServers(incoming); }}
          onClose={() => setShowAdd(false)}
          toneStyle={toneStyleFromElement(pageRef.current)}
        />
      )}
    </div>
  );
}
