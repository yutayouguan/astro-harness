/** 当前 Agent 信息条。 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import { formatDiagnosticContext } from "../lib/diagnosticContext";
import type { AgentInfo } from "../types/agent";
import type { InstalledSkill } from "../types";
import AgentAvatar from "./AgentAvatar";

/** 当前 Agent 信息条入参 */
type Props = {
  sessionId?: string | null;
  turnId?: string | null;
  onOpenMemory: () => void;
  onOpenSkills: () => void;
  /** 打开右侧「上下文」Tab 查看分层占用 */
  onOpenContextTab: () => void;
};

const MEMORY_PREVIEW_LEN = 280;

export default function ChatAgentInfo({
  sessionId = null,
  turnId = null,
  onOpenMemory,
  onOpenSkills,
  onOpenContextTab,
}: Props) {
  const { t } = useI18n();
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const [memoryPreview, setMemoryPreview] = useState("");
  const [diary, setDiary] = useState("");
  const [skillNames, setSkillNames] = useState<string[]>([]);
  const [skillTotal, setSkillTotal] = useState(0);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    const memoryEmpty = t("chat.rightPanel.memoryEmpty");
    const diaryEmpty = t("chat.rightPanel.diaryEmpty");

    void (async () => {
      setLoading(true);
      try {
        const cfg = await invoke<{
          active_agent_id: string;
          agents: AgentInfo[];
        }>("get_config");
        if (cancelled) return;

        const active =
          cfg.agents.find((a) => a.id === cfg.active_agent_id) ?? cfg.agents[0] ?? null;
        setAgent(active);
        if (!active) return;

        try {
          const mem = await invoke<string>("read_file", {
            path: `${active.path}/MEMORY.md`,
          });
          if (cancelled) return;
          const trimmed = mem.trim();
          setMemoryPreview(trimmed ? trimmed.slice(0, MEMORY_PREVIEW_LEN) : memoryEmpty);
        } catch {
          if (!cancelled) setMemoryPreview(memoryEmpty);
        }

        try {
          const dates = await invoke<string[]>("list_daily_memory", {
            agentId: active.id,
          });
          if (cancelled) return;
          if (dates[0]) {
            const content = await invoke<string>("read_daily_memory", {
              agentId: active.id,
              date: dates[0],
            });
            if (cancelled) return;
            setDiary(content.trim() || diaryEmpty);
          } else {
            setDiary(diaryEmpty);
          }
        } catch {
          if (!cancelled) setDiary(diaryEmpty);
        }

        try {
          const skills = await invoke<InstalledSkill[]>("list_installed_skills");
          if (cancelled) return;
          const enabled = skills.filter((s) => s.enabled);
          setSkillTotal(enabled.length);
          setSkillNames(enabled.slice(0, 9).map((s) => s.name));
        } catch {
          if (!cancelled) {
            setSkillNames([]);
            setSkillTotal(0);
          }
        }
      } catch {
        if (!cancelled) setAgent(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [t]);

  if (loading) {
    return <p className="muted">加载 Agent 信息…</p>;
  }

  if (!agent) {
    return <p className="muted">{t("chat.rightPanel.agentUnavailable")}</p>;
  }

  return (
    <div className="chat-agent-info">
      <header className="chat-agent-hero">
        <div className="chat-agent-avatar" aria-hidden>
          <AgentAvatar agent={agent} size={48} />
        </div>
        <div>
          <h3>{agent.name}</h3>
          <p className="muted">
            {agent.is_default ? t("chat.rightPanel.defaultAgent") : agent.id}
          </p>
        </div>
      </header>

      <section className="chat-agent-card">
        <h4>{t("chat.rightPanel.usageTitle")}</h4>
        <button
          type="button"
          className="chat-agent-view-all"
          onClick={onOpenContextTab}
        >
          {t("chat.rightPanel.viewContextUsage")}
        </button>
        {turnId && sessionId ? (
          <div className="chat-agent-turn">
            <span>{t("chat.rightPanel.turnLabel")}</span>
            <code>{turnId.length > 8 ? `${turnId.slice(0, 8)}…` : turnId}</code>
            <button
              type="button"
              className="linkish"
              onClick={() => {
                void navigator.clipboard.writeText(
                  formatDiagnosticContext(sessionId, turnId),
                );
              }}
            >
              {t("chat.rightPanel.copyDiagnostic")}
            </button>
          </div>
        ) : null}
      </section>

      <section className="chat-agent-card">
        <div className="chat-agent-card-head">
          <h4>{t("chat.rightPanel.memory")}</h4>
          <button type="button" className="linkish" onClick={onOpenMemory}>
            {t("chat.rightPanel.edit")}
          </button>
        </div>
        <pre className="chat-agent-preview">{memoryPreview}</pre>
      </section>

      <section className="chat-agent-card">
        <h4>{t("chat.rightPanel.diary")}</h4>
        <pre className="chat-agent-preview">{diary}</pre>
      </section>

      <section className="chat-agent-card">
        <div className="chat-agent-card-head">
          <h4>{t("chat.rightPanel.skills")}</h4>
        </div>
        {skillNames.length > 0 ? (
          <div className="chat-agent-skill-tags">
            {skillNames.map((n) => (
              <span key={n}>{n}</span>
            ))}
          </div>
        ) : (
          <p className="muted">暂无已启用技能</p>
        )}
        <button type="button" className="chat-agent-view-all" onClick={onOpenSkills}>
          {`${t("chat.rightPanel.viewAllSkills")} (${skillTotal})`}
        </button>
      </section>
    </div>
  );
}
