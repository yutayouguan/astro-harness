// 任务监控面板：当前 turn 进度、工具调用流、Todo 计划。

import { useMemo } from "react";
import {
  Activity,
  Bot,
  CheckCircle2,
  Circle,
  Clock,
  Loader2,
  AlertCircle,
  ListTodo,
  XCircle,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { ChatMessage } from "../../types";
import {
  extractLatestTodoPlan,
  type TodoPlan,
} from "../../lib/chat/taskProgress";

type Props = {
  messages: ChatMessage[];
  streaming: boolean;
  /** 当前 turn 工具轮次 */
  toolRound?: number;
  /** 最大工具轮次 */
  maxToolRounds?: number;
  /** 生成速度 tokens/s */
  tokensPerSec?: number;
};

type ActivitySummary = {
  total: number;
  running: number;
  done: number;
  error: number;
};

function summarizeActivities(messages: ChatMessage[]): ActivitySummary {
  let total = 0;
  let running = 0;
  let done = 0;
  let error = 0;
  for (const m of messages) {
    if (m.role !== "assistant" || !m.activities) continue;
    for (const act of m.activities) {
      total++;
      if (act.status === "running") running++;
      else if (act.status === "done") done++;
      else if (act.status === "error") error++;
    }
  }
  return { total, running, done, error };
}

function recentActivities(messages: ChatMessage[], limit = 8) {
  const result: Array<{
    id: string;
    title: string;
    status?: string;
    kind: string;
  }> = [];
  for (let i = messages.length - 1; i >= 0 && result.length < limit; i--) {
    const m = messages[i];
    if (m.role !== "assistant" || !m.activities) continue;
    for (
      let j = m.activities.length - 1;
      j >= 0 && result.length < limit;
      j--
    ) {
      const act = m.activities[j];
      result.push({
        id: act.id,
        title: act.title,
        status: act.status,
        kind: act.kind,
      });
    }
  }
  return result;
}

function StatusIcon({ status }: { status?: string }) {
  switch (status) {
    case "running":
      return (
        <Loader2
          size={13}
          strokeWidth={2}
          className="task-monitor-spin"
          aria-hidden
        />
      );
    case "done":
      return (
        <CheckCircle2
          size={13}
          strokeWidth={2}
          className="task-monitor-icon-done"
          aria-hidden
        />
      );
    case "error":
      return (
        <AlertCircle
          size={13}
          strokeWidth={2}
          className="task-monitor-icon-error"
          aria-hidden
        />
      );
    default:
      return (
        <Circle
          size={13}
          strokeWidth={2}
          className="task-monitor-icon-pending"
          aria-hidden
        />
      );
  }
}

function TodoSection({ plan }: { plan: TodoPlan }) {
  const done = plan.items.filter((it) => it.done).length;
  const total = plan.items.length;
  return (
    <div className="task-monitor-section">
      <div className="task-monitor-section-header">
        <ListTodo size={14} strokeWidth={2} aria-hidden />
        <span>{plan.title}</span>
        <span className="task-monitor-badge">
          {done}/{total}
        </span>
      </div>
      <ul className="task-monitor-todo-list">
        {plan.items.map((item, i) => (
          <li
            key={i}
            className={`task-monitor-todo-item ${item.done ? "is-done" : ""}`}
          >
            {item.done ? (
              <CheckCircle2
                size={13}
                strokeWidth={2}
                className="task-monitor-icon-done"
                aria-hidden
              />
            ) : (
              <Circle
                size={13}
                strokeWidth={2}
                className="task-monitor-icon-pending"
                aria-hidden
              />
            )}
            <span>{item.text}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

export default function TaskMonitorPanel({
  messages,
  streaming,
  toolRound = 0,
  maxToolRounds = 90,
  tokensPerSec,
}: Props) {
  const { t } = useI18n();
  const summary = useMemo(() => summarizeActivities(messages), [messages]);
  const recent = useMemo(() => recentActivities(messages), [messages]);
  const plan = useMemo(() => extractLatestTodoPlan(messages), [messages]);

  return (
    <div className="task-monitor-panel">
      {/* 状态概览 */}
      <div className="task-monitor-stats">
        <div className="task-monitor-stat">
          <Activity size={14} strokeWidth={2} aria-hidden />
          <span className="task-monitor-stat-label">
            {streaming
              ? t("chat.taskMonitor.streaming" as never)
              : t("chat.taskMonitor.idle" as never)}
          </span>
        </div>
        {toolRound > 0 && (
          <div className="task-monitor-stat">
            <Clock size={14} strokeWidth={2} aria-hidden />
            <span className="task-monitor-stat-label">
              轮次 {toolRound}/{maxToolRounds}
            </span>
          </div>
        )}
        {tokensPerSec != null && tokensPerSec > 0 && (
          <div className="task-monitor-stat">
            <span className="task-monitor-stat-label">
              {tokensPerSec.toFixed(1)} tok/s
            </span>
          </div>
        )}
        {summary.total > 0 && (
          <div className="task-monitor-stat">
            <Bot size={14} strokeWidth={2} aria-hidden />
            <span className="task-monitor-stat-label">
              {summary.done}✓{" "}
              {summary.running > 0 ? `${summary.running}⟳ ` : ""}
              {summary.error > 0 ? `${summary.error}✗` : ""}
            </span>
          </div>
        )}
      </div>

      {/* Todo 计划 */}
      {plan && plan.items.length > 0 && <TodoSection plan={plan} />}

      {/* 最近工具调用 */}
      {recent.length > 0 && (
        <div className="task-monitor-section">
          <div className="task-monitor-section-header">
            <Activity size={14} strokeWidth={2} aria-hidden />
            <span>{t("chat.taskMonitor.recentTools" as never)}</span>
          </div>
          <ul className="task-monitor-activity-list">
            {recent.map((act) => (
              <li key={act.id} className="task-monitor-activity-item">
                <StatusIcon status={act.status} />
                <span className="task-monitor-activity-title">{act.title}</span>
                <span className="task-monitor-activity-kind">{act.kind}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* 空状态 */}
      {!streaming && summary.total === 0 && !plan && (
        <div className="task-monitor-empty">
          <XCircle size={32} strokeWidth={1.2} aria-hidden />
          <p>{t("chat.taskMonitor.empty" as never)}</p>
        </div>
      )}
    </div>
  );
}
