/** 当前 Agent 信息条。 */
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  ArrowUpRight,
  BotOff,
  Brain,
  Copy,
  Gauge,
  LoaderCircle,
  NotebookText,
  Pencil,
  Sparkles,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  displayContextWindow,
  formatTokenCount,
  usagePercent,
  type ContextUsageSnapshot,
} from "../../lib/chat/contextUsage";
import { formatDiagnosticContext } from "../../lib/chat/diagnosticContext";
import type { AgentInfo } from "../../types/agent";
import type { InstalledSkill } from "../../types";
import AgentAvatar from "../agents/AgentAvatar";
import ContextUsageBar from "./ContextUsageBar";

/** 当前 Agent 信息条入参 */
type Props = {
  variant?: "all" | "summary" | "context";
  sessionId?: string | null;
  turnId?: string | null;
  /** 分层上下文占用快照 */
  contextUsage?: ContextUsageSnapshot | null;
  /** 模型上下文窗口；未知时由快照回落 */
  contextWindow?: number;
  onOpenMemory: () => void;
  onOpenSkills: () => void;
  /** 打开右侧「上下文」Tab 查看分层占用 */
  onOpenContextTab: () => void;
};

const MEMORY_PREVIEW_LEN = 280;

export default function ChatAgentInfo({
  variant = "all",
  sessionId = null,
  turnId = null,
  contextUsage = null,
  contextWindow = 0,
  onOpenMemory,
  onOpenSkills,
  onOpenContextTab,
}: Props) {
  const { t } = useI18n();
  const [agent, setAgent] = useState<AgentInfo | null>(null);
  const win = displayContextWindow(contextUsage, contextWindow);
  const used = contextUsage?.totalTokens ?? 0;
  const pct = win > 0 ? usagePercent(used, win) : null;
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

        if (variant === "summary") return;

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
  }, [t, variant]);

  if (loading) {
    return (
      <div className="chat-agent-state" role="status">
        <span className="chat-agent-state-icon is-loading" aria-hidden>
          <LoaderCircle size={20} strokeWidth={2} />
        </span>
        <p className="muted">加载 Agent 信息…</p>
      </div>
    );
  }

  if (!agent) {
    return (
      <div className="chat-agent-state" role="status">
        <span className="chat-agent-state-icon" aria-hidden>
          <BotOff size={20} strokeWidth={2} />
        </span>
        <p className="muted">{t("chat.rightPanel.agentUnavailable")}</p>
      </div>
    );
  }

  return (
    <div className="chat-agent-info">
      {variant !== "context" ? (
        <>
          <header className="chat-agent-hero">
            <div className="chat-agent-avatar" aria-hidden>
              <AgentAvatar agent={agent} size={48} />
            </div>
            <div className="chat-agent-identity">
              <h3>{agent.name}</h3>
              <p className="chat-agent-status">
                {agent.is_default ? t("chat.rightPanel.defaultAgent") : agent.id}
              </p>
            </div>
          </header>

          <section className="chat-agent-card">
            <div className="chat-agent-card-head">
              <h4 className="chat-agent-card-title">
                <span className="chat-agent-card-icon" aria-hidden>
                  <Gauge size={15} strokeWidth={2.1} />
                </span>
                <span>{t("chat.rightPanel.usageTitle")}</span>
              </h4>
              <button type="button" className="linkish" onClick={onOpenContextTab}>
                <span>{t("chat.contextUsageDetail")}</span>
                <ArrowUpRight size={12} strokeWidth={2.1} aria-hidden />
              </button>
            </div>
            {contextUsage && (win > 0 || used > 0) ? (
              <button
                type="button"
                className="chat-agent-usage"
                onClick={onOpenContextTab}
                aria-label={
                  pct != null
                    ? t("chat.contextUsageFull", { pct: String(pct) })
                    : t("chat.contextUsage")
                }
              >
                <div className="chat-agent-usage-meta">
                  <span className="chat-agent-usage-pct">
                    {pct != null
                      ? t("chat.contextUsageFull", { pct: String(pct) })
                      : t("chat.contextUsage")}
                  </span>
                  <span className="chat-agent-usage-tokens">
                    ~{formatTokenCount(used)}
                    {win > 0 ? ` / ${formatTokenCount(win)}` : ""}
                  </span>
                </div>
                {win > 0 ? (
                  <ContextUsageBar snapshot={contextUsage} windowTokens={win} />
                ) : null}
              </button>
            ) : (
              <p className="chat-agent-usage-empty muted">
                {t("chat.rightPanel.usagePlaceholder")}
              </p>
            )}
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
                  <Copy size={12} strokeWidth={2.1} aria-hidden />
                  <span>{t("chat.rightPanel.copyDiagnostic")}</span>
                </button>
              </div>
            ) : null}
          </section>
        </>
      ) : null}

      {variant !== "summary" ? (
        <>
          <section className="chat-agent-card">
            <div className="chat-agent-card-head">
              <h4 className="chat-agent-card-title">
                <span className="chat-agent-card-icon" aria-hidden>
                  <Brain size={15} strokeWidth={2.1} />
                </span>
                <span>{t("chat.rightPanel.memory")}</span>
              </h4>
              <button type="button" className="linkish" onClick={onOpenMemory}>
                <Pencil size={12} strokeWidth={2.1} aria-hidden />
                <span>{t("chat.rightPanel.edit")}</span>
              </button>
            </div>
            <pre className="chat-agent-preview">{memoryPreview}</pre>
          </section>

          <section className="chat-agent-card">
            <h4 className="chat-agent-card-title">
              <span className="chat-agent-card-icon" aria-hidden>
                <NotebookText size={15} strokeWidth={2.1} />
              </span>
              <span>{t("chat.rightPanel.diary")}</span>
            </h4>
            <pre className="chat-agent-preview">{diary}</pre>
          </section>

          <section className="chat-agent-card">
            <div className="chat-agent-card-head">
              <h4 className="chat-agent-card-title">
                <span className="chat-agent-card-icon" aria-hidden>
                  <Sparkles size={15} strokeWidth={2.1} />
                </span>
                <span>{t("chat.rightPanel.skills")}</span>
              </h4>
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
              <span>{`${t("chat.rightPanel.viewAllSkills")} (${skillTotal})`}</span>
              <ArrowUpRight size={14} strokeWidth={2.1} aria-hidden />
            </button>
          </section>
        </>
      ) : null}
    </div>
  );
}
