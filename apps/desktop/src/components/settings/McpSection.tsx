/** MCP 区块：Server 列表、详情与新增抽屉；由宿主页面（插件页）提供工具栏槽位。 */
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-shell";
import {
  AlignLeft,
  Braces,
  Check,
  Clock3,
  Download,
  ExternalLink,
  FileText,
  FormInput,
  Globe,
  Heading,
  KeyRound,
  Link2,
  List,
  Plus,
  ShieldAlert,
  ShieldCheck,
  Tag,
  Terminal,
  Timer,
  Trash2,
  X,
} from "lucide-react";
import {
  PUBLIC_MCP_CATALOG,
  installableMcpServer,
} from "../../config/mcpPublicCatalog";
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
  type McpConfigScope,
  type McpPublicCategory,
  type McpToolApprovalMode,
  type McpTransportType,
} from "../../hooks/providers/useMcpTools";
import { useActiveAgent } from "../../hooks/app/useActiveAgent";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import McpIcon from "../icons/McpIcon";
import MotionSwitch from "../ui/MotionSwitch";
import { IconRefresh } from "../icons/NavIcons";
import { SelectMenu } from "../ui/SelectMenu";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";

/** 新增抽屉的两种录入方式 */
type AddTab = "json" | "form";

/** 与宿主页面共用的内容布局 */
type McpView = "gallery" | "list" | "detail";

const MCP_APPROVAL_MODES: McpToolApprovalMode[] = ["auto", "prompt", "writes", "approve"];

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
        scope: "global",
        provenance: "user",
        editable: true,
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
          <MotionSwitch switchKey={addTab} variant="fade">
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
          </MotionSwitch>
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

/** 公开目录卡片只展示模板信息，接入后才生成可编辑的个人配置。 */
function McpCatalogCard({
  server,
  installed,
  installReady,
  onInstall,
}: {
  server: McpServer;
  installed: boolean;
  installReady: boolean;
  onInstall: (server: McpServer) => void;
}) {
  const { t } = useI18n();
  const credentialNames = [
    ...server.envVars,
    ...(server.bearerTokenEnvVar ? [server.bearerTokenEnvVar] : []),
    ...Object.values(server.envHttpHeaders),
  ];

  return (
    <article className="mcp-server-card is-on">
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
        </div>
      </header>
      <p className="mcp-server-desc">{server.description}</p>
      {credentialNames.length > 0 ? (
        <div className="mcp-server-env">
          {credentialNames.map((name) => (
            <span key={name} className="mcp-server-env-key">{name} ← env</span>
          ))}
        </div>
      ) : null}
      <div className="mcp-server-actions">
        {server.websiteUrl ? (
          <button
            type="button"
            className="mcp-btn-ghost"
            onClick={() => void open(server.websiteUrl!)}
          >
            <ExternalLink size={13} aria-hidden />
            {t("mcpTools.documentation")}
          </button>
        ) : null}
        <button
          type="button"
          className="mcp-btn-primary mcp-install-btn"
          disabled={installed || !installReady}
          onClick={() => onInstall(server)}
        >
          {installed ? <Check size={13} aria-hidden /> : <Download size={13} aria-hidden />}
          {installed ? t("mcpTools.installed") : t("mcpTools.install")}
        </button>
      </div>
    </article>
  );
}

/** 宿主页面挂载 MCP 区块所需的入参 */
type McpSectionOptions = {
  /** 区块是否可见（决定是否轮询运行时状态） */
  active: boolean;
  /** 搜索词，由宿主工具栏提供 */
  query: string;
  /** 内容布局，与宿主页面共用 */
  viewMode: McpView;
  /** 取色用的宿主根节点 */
  hostRef: RefObject<HTMLElement | null>;
  scope: McpConfigScope;
  /** 公开目录分类；仅 builtin/public 视图使用。 */
  publicCategory?: McpPublicCategory;
};

/** 宿主页面渲染 MCP 区块所需的片段与状态 */
type McpSection = {
  /** Server 数量，用于 tab 角标 */
  serverCount: number;
  /** 打开新增 Server 抽屉 */
  openAdd: () => void;
  canAdd: boolean;
  /** 主体内容（含新增抽屉） */
  content: ReactNode;
};

/** MCP 区块：状态自持，工具栏由宿主渲染 */
export function useMcpSection({
  active,
  query: rawQuery,
  viewMode,
  hostRef,
  scope,
  publicCategory,
}: McpSectionOptions): McpSection {
  const { t } = useI18n();
  const { activeAgentId: agentId } = useActiveAgent();
  const [showAdd, setShowAdd] = useState(false);
  const [selectedDetailId, setSelectedDetailId] = useState<string | null>(null);
  const {
    servers: configuredServers,
    ready: configuredServersReady,
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
  } = useMcpTools(
    agentId,
    active && scope !== "builtin",
    scope === "builtin" ? "global" : scope,
  );
  const catalogMode = scope === "builtin";
  const servers = catalogMode ? PUBLIC_MCP_CATALOG : configuredServers;
  const installedServerIds = useMemo(
    () => new Set(configuredServers.map((server) => server.id)),
    [configuredServers],
  );
  const installCatalogServer = (server: McpServer) => {
    if (!configuredServersReady || installedServerIds.has(server.id)) return;
    addServers([installableMcpServer(server)]);
  };
  const query = rawQuery.trim().toLowerCase();

  const filteredServers = useMemo(() => {
    return servers.filter((server) => {
      const matchesCategory =
        scope !== "builtin" ||
        !publicCategory ||
        (publicCategory === "featured"
          ? server.featured === true
          : publicCategory === "other"
            ? !server.category || server.category === "other"
            : server.category === publicCategory);
      if (!matchesCategory) return false;
      if (!query) return true;
      return (
        server.name.toLowerCase().includes(query) ||
        server.description.toLowerCase().includes(query) ||
        server.type.toLowerCase().includes(query) ||
        server.command.toLowerCase().includes(query) ||
        server.url.toLowerCase().includes(query) ||
        server.args.join(" ").toLowerCase().includes(query)
      );
    });
  }, [servers, query, scope, publicCategory]);

  useEffect(() => {
    if (viewMode !== "detail") return;
    if (selectedDetailId && filteredServers.some((s) => s.id === selectedDetailId)) {
      return;
    }
    setSelectedDetailId(filteredServers[0]?.id ?? null);
  }, [viewMode, filteredServers, selectedDetailId]);

  const selectedServer = filteredServers.find((s) => s.id === selectedDetailId);
  const selectedRuntimeStatus = !catalogMode && selectedServer
    ? runtimeStatuses[selectedServer.id]
    : undefined;

  const content = (
    <>
          {!catalogMode && runtimeStatusError ? (
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
              <p className="mcp-tools-empty-title">
                {t(`plugins.mcpEmpty.${scope}.title` as MessageKey)}
              </p>
              <p className="mcp-tools-empty-hint">
                {t(`plugins.mcpEmpty.${scope}.hint` as MessageKey)}
              </p>
            </div>
          ) : filteredServers.length === 0 ? (
            <p className="agent-tools-empty">
              {scope === "builtin" && publicCategory
                ? t("plugins.mcpPublic.categoryEmpty")
                : t("agentTools.empty")}
            </p>
          ) : viewMode === "detail" ? (
            <div className="tools-detail">
              <div className="tools-detail-list" role="list">
                {filteredServers.map((server) => (
                  <button
                    key={server.id}
                    type="button"
                    role="listitem"
                    className={`tools-detail-item ${selectedDetailId === server.id ? "is-selected" : ""} ${catalogMode || server.enabled ? "" : "is-disabled"}`}
                    onClick={() => setSelectedDetailId(server.id)}
                  >
                    <span className="tools-detail-item-title">{server.name}</span>
                    <span className="tools-detail-item-meta">
                      {!catalogMode && runtimeStatuses[server.id]
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
                        {!catalogMode ? (
                          <button
                            type="button"
                            role="switch"
                            className="tool-toggle"
                            aria-checked={selectedServer.enabled}
                            onClick={() => toggleServer(selectedServer.id)}
                          >
                            <span className="tool-toggle-thumb" />
                          </button>
                        ) : null}
                      </div>
                      <div className="mcp-server-actions">
                        {catalogMode ? (
                          <>
                            {selectedServer.websiteUrl ? (
                              <button
                                type="button"
                                className="mcp-btn-ghost"
                                onClick={() => void open(selectedServer.websiteUrl!)}
                              >
                                <ExternalLink size={13} aria-hidden />
                                {t("mcpTools.documentation")}
                              </button>
                            ) : null}
                            <button
                              type="button"
                              className="mcp-btn-primary mcp-install-btn"
                              disabled={
                                !configuredServersReady || installedServerIds.has(selectedServer.id)
                              }
                              onClick={() => installCatalogServer(selectedServer)}
                            >
                              {installedServerIds.has(selectedServer.id)
                                ? <Check size={13} aria-hidden />
                                : <Download size={13} aria-hidden />}
                              {installedServerIds.has(selectedServer.id)
                                ? t("mcpTools.installed")
                                : t("mcpTools.install")}
                            </button>
                          </>
                        ) : (
                          <>
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
                          </>
                        )}
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
                    {!catalogMode && selectedRuntimeStatus?.error ? (
                      <p className="mcp-runtime-error" role="alert">
                        <ShieldAlert size={13} strokeWidth={2.2} aria-hidden />
                        <span>{selectedRuntimeStatus.error}</span>
                      </p>
                    ) : null}
                    <McpRetryNote status={selectedRuntimeStatus} />
                    <section className="tools-detail-meta-grid">
                      {!catalogMode ? (
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
                      ) : null}
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
                    {!catalogMode ? <section className="tools-detail-section">
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
                    </section> : null}
                    {!catalogMode ? <section className="tools-detail-section">
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
                    </section> : null}
                  </>
                ) : (
                  <p className="agent-tools-empty">{t("tools.detail.selectHint")}</p>
                )}
              </div>
            </div>
          ) : (
            <div className={`mcp-server-grid is-${viewMode}`}>
              {filteredServers.map((s) => (
                catalogMode ? (
                  <McpCatalogCard
                    key={s.id}
                    server={s}
                    installed={installedServerIds.has(s.id)}
                    installReady={configuredServersReady}
                    onInstall={installCatalogServer}
                  />
                ) : (
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
                )
              ))}
            </div>
          )}
      {showAdd && (
        <McpAddDialog
          onAdd={(incoming) => {
            addServers(incoming);
          }}
          onClose={() => setShowAdd(false)}
          toneStyle={toneStyleFromElement(hostRef.current)}
        />
      )}
    </>
  );

  return {
    serverCount: servers.length,
    openAdd: () => {
      if (scope !== "builtin") setShowAdd(true);
    },
    canAdd: scope !== "builtin",
    content,
  };
}
