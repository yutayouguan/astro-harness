/** MCP 服务器列表与工具启用状态。 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-shell";

/** MCP 传输类型：本地进程 / Streamable HTTP。 */
export type McpTransportType = "stdio" | "streamableHttp";
export type McpRuntimeState =
  | "configured"
  | "disabled"
  | "connecting"
  | "connected"
  | "disconnected"
  | "backoff"
  | "auth-required"
  | "error"
  | "unknown";

export const DEFAULT_MCP_STARTUP_TIMEOUT_SECS = 10;
export const DEFAULT_MCP_TOOL_TIMEOUT_SECS = 60;
export const MAX_MCP_STARTUP_TIMEOUT_SECS = 120;
export const MAX_MCP_TOOL_TIMEOUT_SECS = 3600;

const STARTUP_TIMEOUT_KEYS = [
  "startupTimeoutSecs", "startupTimeoutSec", "startup_timeout_secs", "startup_timeout_sec",
] as const;
const TOOL_TIMEOUT_KEYS = [
  "toolTimeoutSecs", "toolTimeoutSec", "tool_timeout_secs", "tool_timeout_sec",
] as const;

/** 旧 SSE 不能安全地推断为 Streamable HTTP，必须由用户提供新的 /mcp endpoint。 */
export class LegacySseTransportError extends Error {
  constructor() {
    super(
      "Legacy SSE transport is not supported; configure the server's Streamable HTTP /mcp endpoint instead.",
    );
    this.name = "LegacySseTransportError";
  }
}

export type McpDiscoveredTool = {
  name: string;
  description: string;
};

export type McpRuntimeStatus = {
  id: string;
  name: string;
  status: McpRuntimeState;
  tools: string[];
  required: boolean;
  error?: string;
  retryable: boolean;
  retryAttempt: number;
  nextRetryAtUnixMs?: number;
  oauthAvailable: boolean;
  authenticated: boolean;
};

const MCP_RUNTIME_STATES = new Set<McpRuntimeState>([
  "configured",
  "disabled",
  "connecting",
  "connected",
  "disconnected",
  "backoff",
  "auth-required",
  "error",
  "unknown",
]);

export function normalizeMcpRuntimeStatus(
  raw: Partial<McpRuntimeStatus>,
): McpRuntimeStatus {
  const values = raw as Record<string, unknown>;
  const candidate = typeof raw.status === "string" ? raw.status : "unknown";
  const retryAttempt = Number(values.retryAttempt ?? values.retry_attempt ?? 0);
  const nextRetryAt = Number(
    values.nextRetryAtUnixMs ?? values.next_retry_at_unix_ms ?? 0,
  );
  return {
    id: typeof raw.id === "string" ? raw.id : "",
    name: typeof raw.name === "string" ? raw.name : "",
    status: MCP_RUNTIME_STATES.has(candidate as McpRuntimeState)
      ? (candidate as McpRuntimeState)
      : "unknown",
    tools: Array.isArray(raw.tools)
      ? raw.tools.filter((tool): tool is string => typeof tool === "string")
      : [],
    required: raw.required === true,
    error: typeof raw.error === "string" && raw.error.trim() ? raw.error.trim() : undefined,
    retryable: raw.retryable === true,
    retryAttempt: Number.isFinite(retryAttempt) ? Math.max(0, Math.trunc(retryAttempt)) : 0,
    nextRetryAtUnixMs:
      Number.isFinite(nextRetryAt) && nextRetryAt > 0 ? Math.trunc(nextRetryAt) : undefined,
    oauthAvailable: values.oauthAvailable === true || values.oauth_available === true,
    authenticated: values.authenticated === true,
  };
}

function runtimeStatusRecord(
  list: Partial<McpRuntimeStatus>[],
): Record<string, McpRuntimeStatus> {
  const normalized = list.map(normalizeMcpRuntimeStatus);
  return Object.fromEntries(
    normalized
      .filter((status) => status.id)
      .map((status) => [status.id, status]),
  );
}

export type McpServer = {
  id: string;
  name: string;
  description: string;
  type: McpTransportType;
  /** stdio */
  command: string;
  args: string[];
  env: Record<string, string>;
  /** 从本地进程环境按名称转发，不持久化值 */
  envVars: string[];
  /** streamableHttp */
  url: string;
  headers: Record<string, string>;
  /** Bearer Token 所在的环境变量名 */
  bearerTokenEnvVar?: string;
  /** HTTP Header 名到环境变量名的映射 */
  envHttpHeaders: Record<string, string>;
  auth?: "oauth" | "chatgpt";
  enabled: boolean;
  /** 启动失败时阻止 Agent 进入首次 LLM 调用 */
  required: boolean;
  /** STDIO 工作目录，必须位于当前项目执行根内 */
  cwd?: string;
  /** 建连、初始化与首次工具发现的超时（秒） */
  startupTimeoutSecs: number;
  /** 单次工具调用超时（秒） */
  toolTimeoutSecs: number;
  /** 显式 allow list；缺失表示默认允许 */
  enabledTools?: string[];
  /** deny list，在 allow list 之后应用 */
  disabledTools: string[];
  /** 单工具开关；缺失视为启用 */
  tools: Record<string, boolean>;
  /** 最近一次 list_tools 缓存 */
  discovered: McpDiscoveredTool[];
};

export function isMcpToolEnabled(server: McpServer, toolName: string): boolean {
  if (server.enabledTools && !server.enabledTools.includes(toolName)) return false;
  if (server.disabledTools.includes(toolName)) return false;
  return server.tools[toolName] ?? true;
}

const isTauri = () =>
  typeof window !== "undefined" &&
  !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

function readTimeout(
  raw: Record<string, unknown>,
  keys: readonly string[],
  fallback: number,
  max: number,
): number {
  for (const key of keys) {
    const value = raw[key];
    if (value === undefined || value === null || value === "") continue;
    const parsed = typeof value === "number" ? value : Number(value);
    if (Number.isFinite(parsed)) {
      return Math.min(max, Math.max(1, Math.trunc(parsed)));
    }
  }
  return fallback;
}

function readStringArray(
  raw: Record<string, unknown>,
  keys: readonly string[],
): string[] | undefined {
  for (const key of keys) {
    const value = raw[key];
    if (!Array.isArray(value)) continue;
    return value
      .filter((item): item is string => typeof item === "string")
      .map((item) => item.trim())
      .filter(Boolean);
  }
  return undefined;
}

function readStringRecord(
  raw: Record<string, unknown>,
  keys: readonly string[],
): Record<string, string> {
  for (const key of keys) {
    const value = raw[key];
    if (!value || typeof value !== "object" || Array.isArray(value)) continue;
    return Object.fromEntries(
      Object.entries(value as Record<string, unknown>).filter(
        (entry): entry is [string, string] => typeof entry[1] === "string",
      ),
    );
  }
  return {};
}

function readOptionalString(raw: Record<string, unknown>, keys: readonly string[]): string | undefined {
  for (const key of keys) {
    const value = raw[key];
    if (typeof value === "string" && value.trim()) return value.trim();
  }
  return undefined;
}

function normalizeServer(raw: Partial<McpServer> & { id?: string; name?: string }): McpServer {
  const config = raw as unknown as Record<string, unknown>;
  const type = inferType(config);
  const tools =
    raw.tools && typeof raw.tools === "object" ? { ...raw.tools } : ({} as Record<string, boolean>);
  const discovered = Array.isArray(raw.discovered)
    ? raw.discovered.map((d) => ({
        name: d.name ?? "",
        description: d.description ?? "",
      }))
    : [];
  for (const d of discovered) {
    if (d.name && tools[d.name] === undefined) tools[d.name] = true;
  }
  return {
    id: raw.id ?? `mcp_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
    name: raw.name ?? "MCP Server",
    description: raw.description ?? "",
    type,
    command: raw.command ?? "",
    args: Array.isArray(raw.args) ? raw.args : [],
    env: raw.env && typeof raw.env === "object" ? raw.env : {},
    envVars: readStringArray(config, ["envVars", "env_vars"]) ?? [],
    url: raw.url ?? "",
    headers: readStringRecord(config, ["headers", "httpHeaders", "http_headers"]),
    bearerTokenEnvVar: readOptionalString(config, [
      "bearerTokenEnvVar", "bearer_token_env_var",
    ]),
    envHttpHeaders: readStringRecord(config, ["envHttpHeaders", "env_http_headers"]),
    auth:
      readOptionalString(config, ["auth"]) === "chatgpt"
        ? "chatgpt"
        : readOptionalString(config, ["auth"]) === "oauth"
          ? "oauth"
          : undefined,
    enabled: raw.enabled ?? true,
    required: config.required === true,
    cwd: typeof config.cwd === "string" && config.cwd.trim() ? config.cwd.trim() : undefined,
    startupTimeoutSecs: readTimeout(
      config, STARTUP_TIMEOUT_KEYS, DEFAULT_MCP_STARTUP_TIMEOUT_SECS, MAX_MCP_STARTUP_TIMEOUT_SECS,
    ),
    toolTimeoutSecs: readTimeout(
      config, TOOL_TIMEOUT_KEYS, DEFAULT_MCP_TOOL_TIMEOUT_SECS, MAX_MCP_TOOL_TIMEOUT_SECS,
    ),
    enabledTools: readStringArray(config, ["enabledTools", "enabled_tools"]),
    disabledTools: readStringArray(config, ["disabledTools", "disabled_tools"]) ?? [],
    tools,
    discovered,
  };
}

function readLocalStored(): McpServer[] {
  try {
    const raw = localStorage.getItem("mcp-tools");
    if (!raw) return [];
    const list = JSON.parse(raw) as Partial<McpServer>[];
    return Array.isArray(list) ? list.map((s) => normalizeServer(s)) : [];
  } catch {
    return [];
  }
}

function inferType(cfg: Record<string, unknown>): McpTransportType {
  const t = cfg.type ?? cfg.transport;
  if (t === "sse") throw new LegacySseTransportError();
  if (t === "stdio" || t === "streamableHttp" || t === "streamable_http") {
    return t === "streamable_http" ? "streamableHttp" : t;
  }
  if (t !== undefined && t !== null && t !== "") {
    throw new Error(`Unsupported MCP transport: ${String(t)}`);
  }
  if (typeof cfg.url === "string" && cfg.url.trim()) {
    if (/(?:^|\/)sse(?:[/?#]|$)/i.test(cfg.url)) {
      throw new LegacySseTransportError();
    }
    return "streamableHttp";
  }
  return "stdio";
}

export function parseMcpJson(raw: string): McpServer[] {
  const obj = JSON.parse(raw) as Record<string, unknown>;

  // Claude Desktop / Cursor format: { mcpServers: { name: { ... } } }
  if (obj.mcpServers && typeof obj.mcpServers === "object") {
    const servers = obj.mcpServers as Record<string, Record<string, unknown>>;
    return Object.entries(servers).map(([name, cfg]) =>
      normalizeServer({
        name,
        description: typeof cfg.description === "string" ? cfg.description : "",
        type: inferType(cfg),
        command: typeof cfg.command === "string" ? cfg.command : "",
        args: Array.isArray(cfg.args) ? (cfg.args as string[]) : [],
        env:
          cfg.env && typeof cfg.env === "object"
            ? (cfg.env as Record<string, string>)
            : {},
        envVars: readStringArray(cfg, ["envVars", "env_vars"]) ?? [],
        url: typeof cfg.url === "string" ? cfg.url : "",
        headers: readStringRecord(cfg, ["headers", "httpHeaders", "http_headers"]),
        bearerTokenEnvVar: readOptionalString(cfg, [
          "bearerTokenEnvVar", "bearer_token_env_var",
        ]),
        envHttpHeaders: readStringRecord(cfg, ["envHttpHeaders", "env_http_headers"]),
        auth:
          readOptionalString(cfg, ["auth"]) === "chatgpt"
            ? "chatgpt"
            : readOptionalString(cfg, ["auth"]) === "oauth"
              ? "oauth"
              : undefined,
        startupTimeoutSecs: readTimeout(
          cfg, STARTUP_TIMEOUT_KEYS, DEFAULT_MCP_STARTUP_TIMEOUT_SECS, MAX_MCP_STARTUP_TIMEOUT_SECS,
        ),
        toolTimeoutSecs: readTimeout(
          cfg, TOOL_TIMEOUT_KEYS, DEFAULT_MCP_TOOL_TIMEOUT_SECS, MAX_MCP_TOOL_TIMEOUT_SECS,
        ),
        required: cfg.required === true,
        cwd: typeof cfg.cwd === "string" ? cfg.cwd : undefined,
        enabledTools: readStringArray(cfg, ["enabledTools", "enabled_tools"]),
        disabledTools: readStringArray(cfg, ["disabledTools", "disabled_tools"]) ?? [],
        enabled: true,
      }),
    );
  }

  // Single server object
  if (
    typeof obj.command === "string" ||
    typeof obj.url === "string" ||
    typeof obj.name === "string"
  ) {
    return [normalizeServer(obj as Partial<McpServer>)];
  }

  throw new Error("Unrecognized MCP JSON format");
}

export function useMcpTools(agentId?: string | null, watchRuntime = false) {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [ready, setReady] = useState(false);
  const [skipNextSave, setSkipNextSave] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [runtimeStatuses, setRuntimeStatuses] = useState<Record<string, McpRuntimeStatus>>({});
  const [runtimeStatusError, setRuntimeStatusError] = useState<string | null>(null);
  const [reconnectingServerIds, setReconnectingServerIds] = useState<Set<string>>(new Set());
  const [authenticatingServerIds, setAuthenticatingServerIds] = useState<Set<string>>(new Set());

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    setSkipNextSave(true);
    (async () => {
      if (!isTauri()) {
        if (!cancelled) {
          const key = agentId ? `mcp-tools:${agentId}` : "mcp-tools";
          try {
            const raw = localStorage.getItem(key);
            setServers(raw ? (JSON.parse(raw) as McpServer[]).map(normalizeServer) : []);
          } catch {
            setServers([]);
          }
          setReady(true);
        }
        return;
      }
      try {
        const list = await invoke<Partial<McpServer>[]>("get_mcp_servers", {
          agentId: agentId || null,
        });
        if (!cancelled) {
          setServers(list.map((s) => normalizeServer(s)));
          setReady(true);
        }
      } catch {
        if (!cancelled) {
          setServers(readLocalStored());
          setReady(true);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [agentId]);

  const refreshRuntimeStatuses = useCallback(async () => {
    if (!isTauri()) return;
    try {
      const list = await invoke<Partial<McpRuntimeStatus>[]>("get_mcp_server_statuses", {
        agentId: agentId || null,
      });
      setRuntimeStatuses(runtimeStatusRecord(list));
      setRuntimeStatusError(null);
    } catch (error) {
      setRuntimeStatusError(error instanceof Error ? error.message : String(error));
    }
  }, [agentId]);

  useEffect(() => {
    if (!watchRuntime || !isTauri()) {
      setRuntimeStatuses({});
      setRuntimeStatusError(null);
      return;
    }
    void refreshRuntimeStatuses();
    const interval = window.setInterval(() => void refreshRuntimeStatuses(), 5_000);
    return () => window.clearInterval(interval);
  }, [refreshRuntimeStatuses, watchRuntime]);

  const reconnectServer = useCallback(
    async (serverId: string) => {
      if (!isTauri()) return;
      setReconnectingServerIds((current) => new Set(current).add(serverId));
      try {
        const list = await invoke<Partial<McpRuntimeStatus>[]>("reconnect_mcp_server", {
          agentId: agentId || null,
          serverId,
        });
        setRuntimeStatuses(runtimeStatusRecord(list));
        setRuntimeStatusError(null);
      } catch (error) {
        setRuntimeStatusError(error instanceof Error ? error.message : String(error));
      } finally {
        setReconnectingServerIds((current) => {
          const next = new Set(current);
          next.delete(serverId);
          return next;
        });
      }
    },
    [agentId],
  );

  const authenticateServer = useCallback(
    async (serverId: string) => {
      if (!isTauri()) return;
      setAuthenticatingServerIds((current) => new Set(current).add(serverId));
      let flowId: string | undefined;
      try {
        const flow = await invoke<{ flowId: string; authorizationUrl: string }>(
          "begin_mcp_oauth",
          { agentId: agentId || null, serverId },
        );
        flowId = flow.flowId;
        await open(flow.authorizationUrl);
        await invoke("complete_mcp_oauth", { flowId });
        await reconnectServer(serverId);
      } catch (error) {
        if (flowId) {
          await invoke("cancel_mcp_oauth", { flowId }).catch(() => undefined);
        }
        setRuntimeStatusError(error instanceof Error ? error.message : String(error));
      } finally {
        setAuthenticatingServerIds((current) => {
          const next = new Set(current);
          next.delete(serverId);
          return next;
        });
      }
    },
    [agentId, reconnectServer],
  );

  const logoutServer = useCallback(
    async (serverId: string) => {
      if (!isTauri()) return;
      setAuthenticatingServerIds((current) => new Set(current).add(serverId));
      try {
        await invoke("logout_mcp_oauth", { agentId: agentId || null, serverId });
        await reconnectServer(serverId);
      } catch (error) {
        setRuntimeStatusError(error instanceof Error ? error.message : String(error));
      } finally {
        setAuthenticatingServerIds((current) => {
          const next = new Set(current);
          next.delete(serverId);
          return next;
        });
      }
    },
    [agentId, reconnectServer],
  );

  useEffect(() => {
    if (!ready) return;
    if (skipNextSave) {
      setSkipNextSave(false);
      return;
    }
    if (!isTauri()) {
      try {
        const key = agentId ? `mcp-tools:${agentId}` : "mcp-tools";
        localStorage.setItem(key, JSON.stringify(servers));
      } catch {
        // ignore
      }
      return;
    }
    void invoke("set_mcp_servers", { servers, agentId: agentId || null }).catch(() => {});
  }, [servers, ready, agentId, skipNextSave]);

  const addServers = useCallback((incoming: McpServer[]) => {
    setServers((prev) => [...prev, ...incoming.map((s) => normalizeServer(s))]);
  }, []);

  const toggleServer = useCallback((id: string) => {
    setServers((prev) =>
      prev.map((s) => (s.id === id ? { ...s, enabled: !s.enabled } : s)),
    );
  }, []);

  const toggleTool = useCallback((serverId: string, toolName: string) => {
    setServers((prev) =>
      prev.map((s) => {
        if (s.id !== serverId) return s;
        const cur = isMcpToolEnabled(s, toolName);
        const enabledTools =
          !cur && s.enabledTools && !s.enabledTools.includes(toolName)
            ? [...s.enabledTools, toolName]
            : s.enabledTools;
        return {
          ...s,
          enabledTools,
          disabledTools: !cur
            ? s.disabledTools.filter((name) => name !== toolName)
            : s.disabledTools,
          tools: { ...s.tools, [toolName]: !cur },
        };
      }),
    );
  }, []);

  const removeServer = useCallback((id: string) => {
    setServers((prev) => prev.filter((s) => s.id !== id));
  }, []);

  const refreshTools = useCallback(
    async (serverId?: string) => {
      if (!isTauri()) return;
      setRefreshing(true);
      try {
        const list = await invoke<Partial<McpServer>[]>("refresh_mcp_tools", {
          agentId: agentId || null,
          serverId: serverId || null,
        });
        setSkipNextSave(true);
        setServers(list.map((s) => normalizeServer(s)));
      } finally {
        setRefreshing(false);
      }
    },
    [agentId],
  );

  return {
    servers,
    addServers,
    toggleServer,
    toggleTool,
    removeServer,
    refreshTools,
    refreshing,
    runtimeStatuses,
    runtimeStatusError,
    refreshRuntimeStatuses,
    reconnectServer,
    reconnectingServerIds,
    authenticateServer,
    logoutServer,
    authenticatingServerIds,
  };
}
