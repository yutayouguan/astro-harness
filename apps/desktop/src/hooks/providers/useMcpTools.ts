/** MCP 服务器列表与工具启用状态。 */
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** MCP 传输类型：本地进程 / Streamable HTTP。 */
export type McpTransportType = "stdio" | "streamableHttp";

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

export type McpServer = {
  id: string;
  name: string;
  description: string;
  type: McpTransportType;
  /** stdio */
  command: string;
  args: string[];
  env: Record<string, string>;
  /** streamableHttp */
  url: string;
  headers: Record<string, string>;
  enabled: boolean;
  /** 单工具开关；缺失视为启用 */
  tools: Record<string, boolean>;
  /** 最近一次 list_tools 缓存 */
  discovered: McpDiscoveredTool[];
};

const isTauri = () =>
  typeof window !== "undefined" &&
  !!(window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

function normalizeServer(raw: Partial<McpServer> & { id?: string; name?: string }): McpServer {
  const type = inferType(raw as unknown as Record<string, unknown>);
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
    url: raw.url ?? "",
    headers: raw.headers && typeof raw.headers === "object" ? raw.headers : {},
    enabled: raw.enabled ?? true,
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
        url: typeof cfg.url === "string" ? cfg.url : "",
        headers:
          cfg.headers && typeof cfg.headers === "object"
            ? (cfg.headers as Record<string, string>)
            : {},
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

export function useMcpTools(agentId?: string | null) {
  const [servers, setServers] = useState<McpServer[]>([]);
  const [ready, setReady] = useState(false);
  const [skipNextSave, setSkipNextSave] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

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
        const cur = s.tools[toolName] ?? true;
        return { ...s, tools: { ...s.tools, [toolName]: !cur } };
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
  };
}
