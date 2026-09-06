/** App 级当前 Agent：单一数据源，供标题栏与各面板共享。 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import { normalizeAgentId, type AgentInfo } from "../../types/agent";

type AppConfigSlice = {
  active_agent_id: string;
  agents: AgentInfo[];
  workspace_dir?: string;
};

type ActiveAgentContextValue = {
  agents: AgentInfo[];
  activeAgentId: string;
  workspaceDir: string;
  ready: boolean;
  refreshAgents: () => Promise<void>;
  setActiveAgent: (agentId: string) => Promise<string>;
};

const ActiveAgentContext = createContext<ActiveAgentContextValue | null>(null);

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function applyConfig(
  cfg: AppConfigSlice,
  setAgents: (a: AgentInfo[]) => void,
  setActiveAgentId: (id: string) => void,
  setWorkspaceDir: (d: string) => void,
): string {
  const id = normalizeAgentId(cfg.active_agent_id);
  setAgents(cfg.agents ?? []);
  setActiveAgentId(id);
  if (typeof cfg.workspace_dir === "string" && cfg.workspace_dir) {
    setWorkspaceDir(cfg.workspace_dir);
  }
  return id;
}

export function ActiveAgentProvider({ children }: { children: ReactNode }) {
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("default");
  const [workspaceDir, setWorkspaceDir] = useState("");
  const [ready, setReady] = useState(false);
  const switchingRef = useRef(false);

  const refreshAgents = useCallback(async () => {
    if (!isTauri()) {
      setReady(true);
      return;
    }
    try {
      const cfg = await invoke<AppConfigSlice>("get_config");
      applyConfig(cfg, setAgents, setActiveAgentId, setWorkspaceDir);
    } catch {
      // ignore bootstrap errors
    } finally {
      setReady(true);
    }
  }, []);

  useEffect(() => {
    void refreshAgents();
  }, [refreshAgents]);

  useAgentsChanged((payload) => {
    if (switchingRef.current) return;
    void (async () => {
      try {
        const cfg = await invoke<AppConfigSlice>("get_config");
        applyConfig(
          {
            ...cfg,
            active_agent_id: cfg.active_agent_id || payload.active_agent_id,
          },
          setAgents,
          setActiveAgentId,
          setWorkspaceDir,
        );
      } catch {
        setActiveAgentId(normalizeAgentId(payload.active_agent_id));
      }
    })();
  });

  const setActiveAgent = useCallback(
    async (agentId: string) => {
      const next = normalizeAgentId(agentId);
      if (next === activeAgentId && agents.some((a) => a.id === next)) {
        return next;
      }
      const prevId = activeAgentId;
      const prevAgents = agents;
      const prevDir = workspaceDir;
      setActiveAgentId(next);
      if (!isTauri()) return next;

      switchingRef.current = true;
      try {
        const cfg = await invoke<AppConfigSlice>("set_active_agent", {
          agentId: next,
        });
        return applyConfig(cfg, setAgents, setActiveAgentId, setWorkspaceDir);
      } catch (e) {
        setActiveAgentId(prevId);
        setAgents(prevAgents);
        setWorkspaceDir(prevDir);
        throw e;
      } finally {
        switchingRef.current = false;
      }
    },
    [activeAgentId, agents, workspaceDir],
  );

  const value = useMemo(
    () => ({
      agents,
      activeAgentId,
      workspaceDir,
      ready,
      refreshAgents,
      setActiveAgent,
    }),
    [agents, activeAgentId, workspaceDir, ready, refreshAgents, setActiveAgent],
  );

  return (
    <ActiveAgentContext.Provider value={value}>
      {children}
    </ActiveAgentContext.Provider>
  );
}

export function useActiveAgent(): ActiveAgentContextValue {
  const ctx = useContext(ActiveAgentContext);
  if (!ctx) {
    throw new Error("useActiveAgent must be used within ActiveAgentProvider");
  }
  return ctx;
}
