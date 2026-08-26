// 浮动 TODO 进度条：从聊天活动中提取最新计划，输入区上方展示折叠进度。

import { useMemo, useState } from "react";
import { CheckCircle2, Circle, ListTodo } from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import type { ChatMessage } from "../../types";
import { MorphToggleIcon } from "../icons/MorphIcon";

export type TodoPlanItem = {
  text: string;
  done: boolean;
};

export type TodoPlan = {
  title: string;
  planId: string;
  items: TodoPlanItem[];
};

// 扫描助手消息的 activities，提取最后一个 todo 工具调用的计划状态。
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
        /* 跳过格式异常 */
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
        <MorphToggleIcon
          active={expanded}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={14}
          strokeWidth={2.2}
          className="todo-progress-chevron"
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
