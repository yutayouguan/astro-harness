import { useEffect, useState } from "react";
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
        {snapshot.requests.length
          ? `待处理 ${snapshot.requests.length}`
          : `进行中 ${snapshot.tasks.length}`}
        {!connected ? " · 离线" : ""}
      </button>
    );
  const rows = taskRows(snapshot.tasks);
  return (
    <main
      className="pet-task-popup"
      onPointerDown={(e) => {
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
      <header className="pet-task-toolbar">
        <strong>任务与待办 · {snapshot.requests.length}</strong>
        <button
          type="button"
          onClick={() => action(invoke("dismiss_pet_tasks"))}
        >
          稍后处理
        </button>
      </header>
      {error && <p role="alert">{error}</p>}
      {!connected && <p role="status">连接中断，待办正在重新同步…</p>}
      {request && (
        <button className="pet-task-link" onClick={() => selectRequest(null)}>
          返回任务列表
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
              {snapshot.tasks.find((t) => t.sessionId === r.sessionId)?.title ??
                "任务"}
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
    </main>
  );
}
