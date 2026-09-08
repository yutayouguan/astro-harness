import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ExternalLink, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { DesktopPetState } from "../settings/DesktopPetPanel";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import {
  resolveDesktopPetActivity,
  type DesktopPetAnimationState,
  type DesktopPetSessionStatus,
} from "../../lib/ui/desktopPetAnimation";
import DesktopPetCanvas from "./DesktopPetCanvas";

const EMPTY_STATE: DesktopPetState = {
  enabled: false,
  sourcePath: null,
  petPath: null,
  scale: 1,
  alwaysOnTop: true,
  updatedAt: "",
  provider: null,
  model: null,
  spriteVersionNumber: null,
  displayName: null,
  description: null,
};

type SessionStatusPayload = {
  sessionId?: unknown;
  status?: unknown;
  activeFlags?: unknown;
  tsMs?: unknown;
};

type PetActivityPayload = {
  sessionId?: string;
  state?: string;
  tsMs?: number;
};

export default function DesktopPetSurface() {
  const [state, setState] = useState<DesktopPetState>(EMPTY_STATE);
  const [activity, setActivity] = useState<DesktopPetAnimationState>("idle");
  const [dragState, setDragState] = useState<
    "running-left" | "running-right" | null
  >(null);
  const [lookAngle, setLookAngle] = useState<number | null>(null);
  const [reducedMotion, setReducedMotion] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  const statusesRef = useRef<Record<string, DesktopPetSessionStatus>>({});
  const transientTimerRef = useRef<number | null>(null);
  const dragTimerRef = useRef<number | null>(null);
  const visiblePetRef = useRef<{ enabled: boolean; path: string | null }>({
    enabled: false,
    path: null,
  });

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void invoke<DesktopPetState>("get_desktop_pet_state")
      .then((next) => {
        if (!disposed) setState(next);
      })
      .catch(() => {});
    void listen<DesktopPetState>("desktop-pet-changed", (event) => {
      if (!disposed) setState(event.payload);
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else unlisten = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const previous = visiblePetRef.current;
    visiblePetRef.current = { enabled: state.enabled, path: state.petPath };
    const newlyVisible = state.enabled && !previous.enabled;
    const replacedWhileVisible =
      state.enabled && Boolean(previous.path) && previous.path !== state.petPath;
    if (
      state.spriteVersionNumber !== 2 ||
      (!newlyVisible && !replacedWhileVisible)
    ) {
      return;
    }
    if (resolveDesktopPetActivity(statusesRef.current) !== "idle") return;
    if (transientTimerRef.current != null) {
      window.clearTimeout(transientTimerRef.current);
    }
    setActivity("waving");
    transientTimerRef.current = window.setTimeout(() => {
      transientTimerRef.current = null;
      setActivity(resolveDesktopPetActivity(statusesRef.current));
    }, 980);
  }, [state.enabled, state.petPath, state.spriteVersionNumber]);

  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => setReducedMotion(media.matches);
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  useEffect(
    () => () => {
      if (dragTimerRef.current != null) {
        window.clearTimeout(dragTimerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    let disposed = false;
    const cleanups: Array<() => void> = [];
    const clearTransient = () => {
      if (transientTimerRef.current != null) {
        window.clearTimeout(transientTimerRef.current);
        transientTimerRef.current = null;
      }
    };
    const settleToAggregate = (delay: number) => {
      clearTransient();
      transientTimerRef.current = window.setTimeout(() => {
        transientTimerRef.current = null;
        setActivity(resolveDesktopPetActivity(statusesRef.current));
      }, delay);
    };
    const applyStatus = (
      payload: SessionStatusPayload,
      allowTransient = true,
    ) => {
      if (disposed) return;
      const sessionId =
        typeof payload.sessionId === "string" ? payload.sessionId.trim() : "";
      if (!sessionId) return;
      if (
        payload.status !== "idle" &&
        payload.status !== "active" &&
        payload.status !== "systemError"
      ) {
        return;
      }
      const previous = statusesRef.current[sessionId];
      const next: DesktopPetSessionStatus = {
        status: payload.status,
        activeFlags: Array.isArray(payload.activeFlags)
          ? payload.activeFlags.filter(
              (flag): flag is string => typeof flag === "string",
            )
          : [],
        updatedAt: typeof payload.tsMs === "number" ? payload.tsMs : Date.now(),
      };
      if (previous && previous.updatedAt >= next.updatedAt) return;
      statusesRef.current = { ...statusesRef.current, [sessionId]: next };
      const aggregate = resolveDesktopPetActivity(statusesRef.current);
      if (!allowTransient || next.status !== "systemError") {
        clearTransient();
        setActivity(aggregate);
      }
    };
    const setup = async () => {
      try {
        const stopStatus = await listen<SessionStatusPayload>(
          "session_status_changed",
          ({ payload }) => applyStatus(payload),
        );
        if (disposed) stopStatus();
        else cleanups.push(stopStatus);
        const stopActivity = await listen<PetActivityPayload>(
          "desktop_pet_activity_changed",
          ({ payload }) => {
            const aggregate = resolveDesktopPetActivity(statusesRef.current);
            if (payload.state === "review") {
              if (aggregate === "waiting") {
                setActivity("waiting");
                return;
              }
              setActivity("review");
              settleToAggregate(1500);
            } else if (payload.state === "jumping") {
              if (aggregate !== "idle") {
                setActivity(aggregate);
                return;
              }
              setActivity("jumping");
              settleToAggregate(980);
            } else if (payload.state === "failed") {
              setActivity("failed");
              settleToAggregate(2200);
            } else if (payload.state === "waiting") {
              setActivity("waiting");
            } else if (payload.state === "idle") {
              clearTransient();
              setActivity(aggregate);
            }
          },
        );
        if (disposed) stopActivity();
        else cleanups.push(stopActivity);
        const snapshot = await invoke<SessionStatusPayload[]>(
          "list_session_statuses",
        );
        if (disposed) return;
        snapshot.forEach((status) => applyStatus(status, false));
      } catch {
        // The desktop-pet surface can render in browser preview without Tauri IPC.
      }
    };
    void setup();
    return () => {
      disposed = true;
      cleanups.forEach((cleanup) => cleanup());
      clearTransient();
    };
  }, []);

  const petSrc = state.petPath ? resolveMediaSrc(state.petPath) : "";
  const renderedState: DesktopPetAnimationState =
    dragState ?? (activity === "idle" && lookAngle != null ? "look" : activity);

  return (
    <main className="desktop-pet-surface">
      <div
        className="desktop-pet-stage"
        onPointerMove={(event) => {
          if (reducedMotion || activity !== "idle" || dragState) return;
          const rect = event.currentTarget.getBoundingClientRect();
          const dx = event.clientX - (rect.left + rect.width / 2);
          const dy = event.clientY - (rect.top + rect.height / 2);
          if (Math.hypot(dx, dy) < 28) {
            setLookAngle(null);
            return;
          }
          setLookAngle(((Math.atan2(dx, -dy) * 180) / Math.PI + 360) % 360);
        }}
        onPointerLeave={() => setLookAngle(null)}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          if (!reducedMotion) {
            const rect = event.currentTarget.getBoundingClientRect();
            setDragState(
              event.clientX < rect.left + rect.width / 2
                ? "running-left"
                : "running-right",
            );
            if (dragTimerRef.current != null) {
              window.clearTimeout(dragTimerRef.current);
            }
            dragTimerRef.current = window.setTimeout(() => {
              dragTimerRef.current = null;
              setDragState(null);
            }, 850);
          }
          void getCurrentWindow().startDragging();
        }}
        onDoubleClick={() => void invoke("open_desktop_pet_main")}
      >
        {petSrc && state.spriteVersionNumber === 2 ? (
          <DesktopPetCanvas
            src={petSrc}
            state={renderedState}
            lookAngle={lookAngle}
            reducedMotion={reducedMotion}
            className="desktop-pet-character desktop-pet-character--canvas"
            label={state.displayName || "Animated desktop pet"}
          />
        ) : petSrc ? (
          <img
            className="desktop-pet-character"
            src={petSrc}
            alt="Desktop pet"
            draggable={false}
          />
        ) : (
          <div className="desktop-pet-placeholder" aria-label="Desktop pet">
            <span>🐾</span>
          </div>
        )}
        <div className="desktop-pet-actions" data-tauri-drag-region="false">
          <button
            type="button"
            aria-label="Open Astro"
            title="Open Astro"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() => void invoke("open_desktop_pet_main")}
          >
            <ExternalLink size={14} />
          </button>
          <button
            type="button"
            aria-label="Hide desktop pet"
            title="Hide desktop pet"
            onPointerDown={(event) => event.stopPropagation()}
            onClick={() =>
              void invoke("set_desktop_pet_enabled", { enabled: false })
            }
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </main>
  );
}
