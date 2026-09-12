import { useEffect, useRef, useState } from "react";
import { ArrowLeft, ArrowUpRight, Inbox, PawPrint } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  advanceMorphSpring,
  type PetTaskMorphFrame,
} from "../../lib/chat/petTaskMorph";
import {
  usePendingInteractions,
  openInteractionSession,
} from "../../hooks/chat/usePendingInteractions";
import InteractionCard from "./InteractionCard";
import { taskRows } from "../../lib/chat/pendingInteractions";

function surfaceZoom() {
  const zoom = Number.parseFloat(document.documentElement.style.zoom);
  return Number.isFinite(zoom) && zoom > 0 ? zoom : 1;
}

export default function PetTaskSurface({
  preview = false,
}: {
  preview?: boolean;
}) {
  const { connected, snapshot, selected, expanded } = usePendingInteractions();
  const [error, setError] = useState("");
  const [localKey, setLocalKey] = useState<string | null>(null);
  const content = useRef<HTMLDivElement>(null);
  const shell = useRef<HTMLDivElement>(null);
  const panel = useRef<HTMLElement>(null);
  const target = useRef({ expanded, height: 420, reduced: false });
  target.current.expanded = expanded;
  useEffect(() => {
    if (panel.current) panel.current.inert = !expanded;
  }, [expanded]);
  useEffect(() => {
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    const update = () => {
      target.current.reduced = query.matches;
      if (!preview)
        void invoke("set_pet_task_motion_preference", {
          reduced: query.matches,
        }).catch(() => {});
    };
    update();
    query.addEventListener("change", update);
    return () => query.removeEventListener("change", update);
  }, [preview]);
  useEffect(() => {
    let disposed = false,
      stop: (() => void) | undefined,
      sequence = -1;
    const apply = (frame: PetTaskMorphFrame) => {
      if (disposed || frame.sequence < sequence || !shell.current) return;
      sequence = frame.sequence;
      const zoom = surfaceZoom();
      shell.current.style.setProperty("--pet-shell-radius", `${19 / zoom}px`);
      shell.current.style.setProperty("--pet-capsule-width", `${166 / zoom}px`);
      shell.current.style.setProperty("--pet-capsule-height", `${38 / zoom}px`);
      shell.current.style.setProperty(
        "--pet-morph",
        String(Math.max(0, Math.min(1, frame.progress))),
      );
      shell.current.dataset.grow = frame.left ? "left" : "right";
      shell.current.dataset.motion =
        frame.progress > 0 && frame.progress < 1 ? "moving" : "still";
      shell.current.style.setProperty(
        "--pet-content-width",
        `${(frame.contentWidth ?? 392) / zoom}px`,
      );
      shell.current.style.setProperty(
        "--pet-content-height",
        `${(frame.contentHeight ?? 560) / zoom}px`,
      );
    };
    if (!preview) {
      void listen<PetTaskMorphFrame>("pet-task-morph-frame", ({ payload }) =>
        apply(payload),
      ).then((unlisten) => {
        if (disposed) {
          unlisten();
          return;
        }
        stop = unlisten;
        void invoke<PetTaskMorphFrame>("get_pet_task_morph_frame")
          .then(apply)
          .catch(() => {});
      });
      return () => {
        disposed = true;
        stop?.();
      };
    }
    // Storybook drives the same live-value spring and renders the real content.
    let frame = 0,
      last = performance.now(),
      value = target.current.expanded ? 1 : 0,
      velocity = 0;
    let width = target.current.expanded ? 392 : 166,
      height = target.current.expanded ? target.current.height : 38,
      vw = 0,
      vh = 0;
    const tick = (now: number) => {
      const dt = (now - last) / 1000;
      last = now;
      const { expanded, height: contentHeight, reduced } = target.current;
      if (reduced) {
        value = expanded ? 1 : 0;
        width = expanded ? 392 : 166;
        height = expanded ? contentHeight : 38;
        velocity = vw = vh = 0;
      } else {
        ({ value, velocity } = advanceMorphSpring(
          value,
          velocity,
          expanded ? 1 : 0,
          dt,
        ));
        const w = advanceMorphSpring(width, vw, expanded ? 392 : 166, dt);
        width = w.value;
        vw = w.velocity;
        const h = advanceMorphSpring(
          height,
          vh,
          expanded ? contentHeight : 38,
          dt,
        );
        height = h.value;
        vh = h.velocity;
        if (
          Math.abs(value - (expanded ? 1 : 0)) < 0.002 &&
          Math.abs(velocity) < 0.02
        ) {
          value = expanded ? 1 : 0;
          velocity = 0;
        }
        if (
          Math.abs(width - (expanded ? 392 : 166)) < 0.2 &&
          Math.abs(vw) < 1
        ) {
          width = expanded ? 392 : 166;
          vw = 0;
        }
        if (
          Math.abs(height - (expanded ? contentHeight : 38)) < 0.2 &&
          Math.abs(vh) < 1
        ) {
          height = expanded ? contentHeight : 38;
          vh = 0;
        }
      }
      if (shell.current) {
        shell.current.style.width = `${width / surfaceZoom()}px`;
        shell.current.style.height = `${height / surfaceZoom()}px`;
      }
      apply({
        sequence: ++sequence,
        progress: value,
        left: true,
        width,
        height,
        contentWidth: 392,
        contentHeight,
      });
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => {
      disposed = true;
      cancelAnimationFrame(frame);
    };
  }, [preview]);
  useEffect(() => {
    if (!content.current) return;
    let frame = 0;
    let lastHeight = 0;
    const report = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        const height = Math.ceil(
          (content.current?.getBoundingClientRect().height ?? 0) +
            32 * surfaceZoom(),
        );
        if (height === lastHeight) return;
        lastHeight = height;
        target.current.height = Math.max(180, Math.min(560, height));
        if (!preview)
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
  }, [preview]);
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
  const rows = taskRows(snapshot.tasks);
  return (
    <div
      ref={shell}
      className={`pet-task-morph${preview ? " is-preview" : ""}`}
      data-expanded={expanded}
    >
      <div className="pet-task-collapsed" aria-hidden={expanded}>
        <button
          className="pet-task-badge"
          tabIndex={expanded ? -1 : 0}
          aria-expanded={expanded}
          onClick={() => action(invoke("open_pet_tasks", { requestKey: null }))}
        >
          <span className="pet-task-badge-copy">
            <PawPrint size={15} aria-hidden />
            <span>
              {snapshot.requests.length
                ? `待处理 ${snapshot.requests.length}`
                : `进行中 ${snapshot.tasks.length}`}
              {!connected ? " · 离线" : ""}
            </span>
            <ArrowUpRight size={13} aria-hidden />
          </span>
        </button>
      </div>
      <main
        ref={panel}
        aria-hidden={!expanded}
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
    </div>
  );
}
