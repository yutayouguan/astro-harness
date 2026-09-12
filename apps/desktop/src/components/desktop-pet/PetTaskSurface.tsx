import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowUpRight, Inbox, PawPrint } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import {
  usePendingInteractions,
  openInteractionSession,
} from "../../hooks/chat/usePendingInteractions";
import InteractionCard from "./InteractionCard";
import { taskRows } from "../../lib/chat/pendingInteractions";

export default function PetTaskSurface({ badge }: { badge: boolean }) {
  const { connected, snapshot, selected } = usePendingInteractions();
  const [error, setError] = useState("");
  const [localKey, setLocalKey] = useState<string | null>(null);
  const content = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (badge || !content.current) return;
    let frame = 0;
    let lastHeight = 0;
    const report = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const height = Math.ceil(
          (content.current?.getBoundingClientRect().height ?? 0) + 32,
        );
        if (height === lastHeight) return;
        lastHeight = height;
        void invoke("resize_pet_task_content", { height }).catch(() => {});
      });
    };
    const observer = new ResizeObserver(report);
    observer.observe(content.current);
    report();
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, [badge]);
  useEffect(() => setLocalKey(selected), [selected]);
  const request = snapshot.requests.find((r) => r.key === localKey);
  const action = (promise: Promise<unknown>) => {
    void promise.catch((e) => setError(String(e)));
  };
  const selectRequest = (key: string | null) => {
    setLocalKey(key);
    action(invoke("open_pet_tasks", { requestKey: key }));
  };
  useEffect(() => {
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") void invoke("dismiss_pet_tasks");
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, []);
  if (badge)
    return (
      <button
        className="pet-task-badge"
        onClick={() => action(invoke("open_pet_tasks", { requestKey: null }))}
      >
        <PawPrint size={15} aria-hidden />
        <span>
          {snapshot.requests.length
            ? `待处理 ${snapshot.requests.length}`
            : `进行中 ${snapshot.tasks.length}`}
          {!connected ? " · 离线" : ""}
        </span>
        <ArrowUpRight size={13} aria-hidden />
      </button>
    );
  const rows = taskRows(snapshot.tasks);
  return (
    <main
      className="pet-task-popup"
      onPointerDown={(e) => {
        // Only acquire native keyboard focus on the first click from outside.
        // Cancelling every pointerdown also cancels caret placement and selection.
        if (e.button !== 0 || document.hasFocus()) return;
        const field = (e.target as Element).closest<HTMLElement>(
          "input,textarea,select",
        );
        if (field) {
          if (field.tagName !== "SELECT") e.preventDefault();
          action(
            invoke("focus_pet_tasks").then(() => {
              if (field.isConnected) field.focus();
            }),
          );
        }
      }}
    >
      <div ref={content} className="pet-task-content">
        <header className="pet-task-toolbar">
          <div className="pet-task-heading">
            <span className="pet-task-mark">
              <Inbox size={18} aria-hidden />
            </span>
            <div>
              <strong>任务与待办</strong>
              <small>
                {snapshot.requests.length
                  ? `${snapshot.requests.length} 项需要你处理`
                  : "查看当前任务进展"}
              </small>
            </div>
          </div>
          <button
            className="pet-task-quiet"
            type="button"
            onClick={() => action(invoke("dismiss_pet_tasks"))}
          >
            稍后处理
          </button>
        </header>
        {error && <p role="alert">{error}</p>}
        {!connected && <p role="status">连接中断，待办正在重新同步…</p>}
        {request && (
          <button
            className="pet-task-link pet-task-back"
            onClick={() => selectRequest(null)}
          >
            <ArrowLeft size={14} aria-hidden /> 返回任务列表
          </button>
        )}
        <div hidden={!!request}>
          {snapshot.requests.map((r) => (
            <button
              className="pet-task-row"
              key={r.key}
              onClick={() => selectRequest(r.key)}
            >
              <span>{r.kind === "approval" ? "待审批" : "待回答"}</span>
              <strong>
                {snapshot.tasks.find((t) => t.sessionId === r.sessionId)
                  ?.title ?? "任务"}
              </strong>
            </button>
          ))}
          {rows.map(({ task, depth }) => (
            <button
              key={task.sessionId}
              className="pet-task-row"
              style={{ paddingLeft: 12 + Math.min(depth, 4) * 14 }}
              onClick={() => action(openInteractionSession(task))}
            >
              <span>{task.status === "waiting" ? "等待处理" : "运行中"}</span>
              <strong>{task.title}</strong>
            </button>
          ))}
          {!snapshot.tasks.length && <p>当前没有进行中的任务。</p>}
        </div>
        {snapshot.requests.map((r) => (
          <div hidden={r.key !== request?.key} key={r.key}>
            <InteractionCard request={r} />
          </div>
        ))}
      </div>
    </main>
  );
}
