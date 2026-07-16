/** 近期会话列表。 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Plus } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useAgentsChanged } from "../../lib/agent/agentsChanged";
import type { RecentSessionDto } from "../../types";
import type { AgentInfo } from "../../types/agent";
import { normalizeAgentId } from "../../types/agent";
import AgentPicker from "../agents/AgentPicker";
import ExpandableSearch from "../ui/ExpandableSearch";

/** 近期会话列表入参 */
type Props = {
  /** 当前打开的会话（高亮） */
  activeSessionId: string | null;
  onOpenSession: (sessionId: string) => void;
  /** 新建空白会话 */
  onNewSession: () => void;
  /** 新建 Agent 引导 */
  onNewAgent: () => void;
};

export default function ChatSessionList({
  activeSessionId,
  onOpenSession,
  onNewSession,
  onNewAgent,
}: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<RecentSessionDto[]>([]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [agents, setAgents] = useState<AgentInfo[]>([]);
  const [activeAgentId, setActiveAgentId] = useState("workspace");

  const loadSessions = useCallback(async () => {
    try {
      const list = await invoke<RecentSessionDto[]>("list_recent_sessions", {
        limit: 50,
      });
      setItems(list ?? []);
      setError(null);
    } catch (e) {
      setError(String(e));
      setItems([]);
    }
  }, []);

  const loadAgents = useCallback(async () => {
    try {
      const cfg = await invoke<{
        active_agent_id: string;
        agents: AgentInfo[];
      }>("get_config");
      setAgents(cfg.agents ?? []);
      setActiveAgentId(normalizeAgentId(cfg.active_agent_id));
    } catch {
      setAgents([]);
    }
  }, []);

  useEffect(() => {
    void loadSessions();
    void loadAgents();
  }, [loadSessions, loadAgents]);

  useAgentsChanged((payload) => {
    setActiveAgentId(normalizeAgentId(payload.active_agent_id));
    void loadAgents();
  });

  const handleAgentChange = useCallback(
    async (id: string) => {
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("set_active_agent", { agentId: id });
        setAgents(cfg.agents ?? []);
        setActiveAgentId(normalizeAgentId(cfg.active_agent_id));
      } catch (e) {
        console.warn("set_active_agent failed", e);
      }
    },
    [],
  );

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return items;
    return items.filter(
      (s) =>
        (s.summary ?? "").toLowerCase().includes(q) ||
        s.sessionId.toLowerCase().includes(q),
    );
  }, [items, query]);

  return (
    <div className="chat-session-list">
      <div className="chat-session-toolbar">
        <ExpandableSearch
          value={query}
          onChange={setQuery}
          placeholderKey="chat.rightPanel.searchSessions"
          className="chat-session-search"
        />
        <button
          type="button"
          className="chat-session-new"
          onClick={onNewSession}
        >
          <Plus size={15} strokeWidth={2.2} aria-hidden />
          {t("chat.newSession")}
        </button>
        <AgentPicker
          agents={agents}
          value={activeAgentId}
          onChange={(id) => void handleAgentChange(id)}
          onCreateNew={onNewAgent}
          labelKey="chat.rightPanel.agent"
          className="chat-session-agent-picker"
        />
      </div>
      {error && <div className="side-error">{error}</div>}
      {filtered.length === 0 ? (
        <p className="muted">{t("chat.rightPanel.noSessions")}</p>
      ) : (
        <ul>
          {filtered.map((s) => (
            <li key={s.sessionId}>
              <button
                type="button"
                className={`chat-session-item ${
                  s.sessionId === activeSessionId ? "is-active" : ""
                }`}
                onClick={() => onOpenSession(s.sessionId)}
              >
                <strong>
                  {(s.summary ?? "").trim() ||
                    t("chat.rightPanel.untitledSession")}
                  {s.endReason === "compacted" ? (
                    <span className="chat-session-badge">
                      {t("chat.sessionCompactedBadge")}
                    </span>
                  ) : null}
                </strong>
                <span>{s.sessionId.slice(0, 8)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
