// 输入框上方任务状态：悬停查看 TODO 清单与本轮文件改动。

import { useMemo, useState, type CSSProperties, type FocusEvent } from "react";
import { CheckCircle2, Circle, Files, ListTodo } from "lucide-react";
import type { ConversationEntry } from "../../types";
import {
  displayFileName,
  extractFileChangeSummary,
  extractLatestTodoPlan,
  type FileChangeItem,
} from "../../lib/chat/taskProgress";

type Props = {
  messages: ConversationEntry[];
  onOpenFileReview?: (file: FileChangeItem, files: FileChangeItem[]) => void;
};

export default function TodoProgress({ messages, onOpenFileReview }: Props) {
  const plan = useMemo(() => extractLatestTodoPlan(messages), [messages]);
  const fileChanges = useMemo(
    () => extractFileChangeSummary(messages),
    [messages],
  );
  const [openPanel, setOpenPanel] = useState<"todo" | "files" | null>(null);

  if (!plan || plan.items.length === 0) return null;

  const total = plan.items.length;
  const done = plan.items.filter((it) => it.done).length;
  const currentStep = done + 1;
  const allDone = done === total;
  if (allDone) return null;
  const currentItem = allDone ? plan.title : plan.items[done]?.text;
  const progressPct = Math.round((done / total) * 100);
  const progressStyle = {
    "--todo-progress": `${progressPct * 3.6}deg`,
  } as CSSProperties;

  const closeOnBlur = (event: FocusEvent<HTMLElement>) => {
    if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
      setOpenPanel(null);
    }
  };

  return (
    <div
      className="todo-progress-float"
      role="status"
      aria-label={`TODO ${done}/${total}`}
      onMouseLeave={() => setOpenPanel(null)}
    >
      <section
        className="todo-progress-section is-todo"
        onMouseEnter={() => setOpenPanel("todo")}
        onBlur={closeOnBlur}
      >
        <button
          type="button"
          className="todo-progress-toggle"
          aria-expanded={openPanel === "todo"}
          aria-haspopup="dialog"
          onClick={() =>
            setOpenPanel((open) => (open === "todo" ? null : "todo"))
          }
        >
          <span
            className="todo-progress-ring"
            style={progressStyle}
            aria-hidden
          >
            <span />
          </span>
          <span className="todo-progress-step">
            {allDone
              ? `${total} / ${total} 步已完成`
              : `第 ${currentStep} / ${total} 步`}
          </span>
          {currentItem ? (
            <>
              <span className="todo-progress-current-separator" aria-hidden>
                ·
              </span>
              <span className="todo-progress-current">{currentItem}</span>
            </>
          ) : null}
        </button>

        {openPanel === "todo" && (
          <div
            className="todo-progress-popover is-todo"
            role="dialog"
            aria-label="TODO 列表"
          >
            <header className="todo-progress-popover-title">
              <ListTodo size={14} strokeWidth={2} aria-hidden />
              <span>{plan.title}</span>
              <span>
                {done}/{total}
              </span>
            </header>
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
          </div>
        )}
      </section>

      {fileChanges.items.length > 0 && (
        <>
          <span className="todo-progress-separator" aria-hidden>
            ·
          </span>
          <section
            className="todo-progress-section is-files"
            onMouseEnter={() => setOpenPanel("files")}
            onBlur={closeOnBlur}
          >
            <button
              type="button"
              className="todo-progress-toggle"
              aria-expanded={openPanel === "files"}
              aria-haspopup="dialog"
              onClick={() =>
                setOpenPanel((open) => (open === "files" ? null : "files"))
              }
            >
              <span>{fileChanges.items.length} 个文件已更改</span>
              {fileChanges.additions > 0 && (
                <span className="todo-progress-additions">
                  +{fileChanges.additions.toLocaleString()}
                </span>
              )}
              {fileChanges.deletions > 0 && (
                <span className="todo-progress-deletions">
                  −{fileChanges.deletions.toLocaleString()}
                </span>
              )}
            </button>

            {openPanel === "files" && (
              <div
                className="todo-progress-popover is-files"
                role="dialog"
                aria-label="文件改动记录"
              >
                <header className="todo-progress-popover-title">
                  <Files size={14} strokeWidth={2} aria-hidden />
                  <span>文件改动</span>
                  <span>{fileChanges.items.length}</span>
                </header>
                <ul className="todo-progress-file-list">
                  {fileChanges.items.map((item) => (
                    <li key={item.path}>
                      <button
                        type="button"
                        className="todo-progress-file-item"
                        title={`审查 ${item.path}`}
                        onClick={() => {
                          setOpenPanel(null);
                          onOpenFileReview?.(item, fileChanges.items);
                        }}
                      >
                        <span>{displayFileName(item.path)}</span>
                        <span className="todo-progress-file-stat">
                          {item.additions > 0 && (
                            <span className="todo-progress-additions">
                              +{item.additions}
                            </span>
                          )}
                          {item.deletions > 0 && (
                            <span className="todo-progress-deletions">
                              −{item.deletions}
                            </span>
                          )}
                        </span>
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </section>
        </>
      )}
    </div>
  );
}
