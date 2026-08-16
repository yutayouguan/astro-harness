/**
 * 浮动 TODO 进度条：从聊天活动中提取最新的 todo 计划，
 * 在输入区上方展示折叠进度摘要，点击展开完整步骤列表。
 */

import { useMemo, useState } from "react";
import { CheckCircle2, Circle, ChevronDown, ListTodo } from "lucide-react";
import type { ChatMessage } from "../../types";

export type TodoPlanItem = {
  text: string;
  done: boolean;
};

export type TodoPlan = {
  title: string;
  planId: string;
  items: TodoPlanItem[];
};

/**
 * 从消息列表中提取最新的 todo 计划。
 * 扫描所有助手消息的 activities，找到最后一个 todo 工具调用并解析其 input。
 */
export function extractLatestTodoPlan(
  messages: ChatMessage[],
): TodoPlan | null {
  let latest: TodoPlan | null = null;

  for (const m of messages) {
    if (m.role !== "assistant" || !m.activities) continue;
    for (const act of m.activities) {
      if (act.title !== "todo" || !act.input) continue;
      try {
        const args = JSON.parse(act.input);
        if (!Array.isArray(args.items) || args.items.length === 0) continue;
        const items: TodoPlanItem[] = args.items.map(
          (it: string | { text: string; done?: boolean }) => {
            if (typeof it === "string") return { text: it, done: false };
            return { text: it.text, done: Boolean(it.done) };
          },
        );
        const planId =
          args.plan_id ||
          (act.output?.match(/([0-9]{8}-[a-f0-9]{6})/)?.[1] ?? "");
        latest = {
          title: args.title || "Todo",
          planId,
          items,
        };
      } catch {
        /* skip malformed */
      }
    }
  }

  return latest;
}

type Props = {
  messages: ChatMessage[];
};

export default function TodoProgress({ messages }: Props) {
  const plan = useMemo(() => extractLatestTodoPlan(messages), [messages]);
  const [expanded, setExpanded] = useState(false);

  if (!plan || plan.items.length === 0) return null;

  const total = plan.items.length;
  const done = plan.items.filter((it) => it.done).length;
  const currentStep = done + 1;
  const allDone = done === total;
  const progressPct = Math.round((done / total) * 100);

  return (
    <div className="todo-progress-float">
      <button
        type="button"
        className="todo-progress-toggle"
        aria-expanded={expanded}
        onClick={() => setExpanded((o) => !o)}
      >
        <ChevronDown
          size={14}
          strokeWidth={2.2}
          className={expanded ? "is-open" : ""}
          aria-hidden
        />
        <ListTodo size={14} strokeWidth={2} aria-hidden />
        <span className="todo-progress-summary">
          {plan.title}
          <span className="todo-progress-step">
            {allDone
              ? `${total}/${total} 步已完成`
              : `第 ${currentStep}/${total} 步`}
          </span>
        </span>
        <span className="todo-progress-bar-wrap" aria-hidden>
          <span
            className={`todo-progress-bar-fill ${allDone ? "is-done" : ""}`}
            style={{ width: `${progressPct}%` }}
          />
        </span>
      </button>

      {expanded && (
        <ul className="todo-progress-list">
          {plan.items.map((item, i) => (
            <li
              key={i}
              className={`todo-progress-item ${item.done ? "is-done" : ""} ${
                !item.done && i === done ? "is-current" : ""
              }`}
            >
              {item.done ? (
                <CheckCircle2
                  size={15}
                  strokeWidth={2.2}
                  className="todo-progress-icon is-check"
                  aria-hidden
                />
              ) : (
                <Circle
                  size={15}
                  strokeWidth={2}
                  className="todo-progress-icon"
                  aria-hidden
                />
              )}
              <span className="todo-progress-item-text">{item.text}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
