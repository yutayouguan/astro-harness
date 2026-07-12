/** 当前 Agent 信息条。 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../i18n/LocaleContext";
import type { InstalledSkill } from "../types";

type AgentInfo = {
  id: string;
  name: string;
  path: string;
  is_default: boolean;
  is_active: boolean;
};

export type TokenUsage = {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
};

/** 当前 Agent 信息条入参 */
type Props = {
  onOpenMemory: () => void;
  onOpenSkills: () => void;
  /** 本轮 token 用量摘要 */
  tokenUsage?: TokenUsage | null;
};

const MEMORY_PREVIEW_LEN = 280;

export default function ChatAgentInfo({
  onOpenMemory,
  onOpenSkills,
  tokenUsage = null,
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

  const hasUsage = !!tokenUsage && tokenUsage.totalTokens > 0;
  const usageRatio = hasUsage
    ? Math.min(1, tokenUsage.totalTokens / 128_000)
    : 0;

  return (
    <div className="chat-agent-info">
      <header className="chat-agent-hero">
        <div className="chat-agent-avatar" aria-hidden>
          {agent.name.slice(0, 1).toUpperCase()}
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
        {hasUsage ? (
          <ul className="chat-agent-usage-stats">
            <li>
              <span>{t("chat.rightPanel.usagePrompt")}</span>
              <strong>{tokenUsage.promptTokens}</strong>
            </li>
            <li>
              <span>{t("chat.rightPanel.usageCompletion")}</span>
              <strong>{tokenUsage.completionTokens}</strong>
            </li>
            <li>
              <span>{t("chat.rightPanel.usageTotal")}</span>
              <strong>{tokenUsage.totalTokens}</strong>
            </li>
          </ul>
        ) : (
          <p className="muted">{t("chat.rightPanel.usagePlaceholder")}</p>
        )}
        <div
          className="chat-agent-usage-bar"
          aria-hidden
          style={
            hasUsage
              ? {
                  background: `linear-gradient(90deg, var(--tone-blue, #2563eb) ${
                    usageRatio * 100
                  }%, color-mix(in srgb, var(--ink) 8%, transparent) ${
                    usageRatio * 100
                  }%)`,
                }
              : undefined
          }
        />
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
