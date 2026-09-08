import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ExternalLink, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { useDesktopPetState } from "../../hooks/app/useDesktopPetState";
import {
  applyPetActivity,
  applyPetSessionStatus,
  emptyPetRuntime,
  resolvePetRuntime,
} from "../../lib/ui/desktopPetRuntime";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { type DesktopPetAnimationState } from "../../lib/ui/desktopPetAnimation";
import DesktopPetCanvas from "./DesktopPetCanvas";

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
  const { state, error, mutate } = useDesktopPetState();
  const [activity, setActivity] = useState<DesktopPetAnimationState>("idle");
  const [dragState, setDragState] = useState<
    "running-left" | "running-right" | null
  >(null);
  const [lookAngle, setLookAngle] = useState<number | null>(null);
  const [reducedMotion, setReducedMotion] = useState(
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
  );
  const runtimeRef = useRef(emptyPetRuntime());
  const transientTimerRef = useRef<number | null>(null);
  const dragTimerRef = useRef<number | null>(null);
  const visiblePetRef = useRef<{ enabled: boolean; path: string | null }>({
    enabled: false,
    path: null,
  });

  useEffect(() => {
    const previous = visiblePetRef.current;
    visiblePetRef.current = { enabled: state.enabled, path: state.petPath };
    const newlyVisible = state.enabled && !previous.enabled;
    const replacedWhileVisible =
      state.enabled &&
      Boolean(previous.path) &&
      previous.path !== state.petPath;
    if (
      state.spriteVersionNumber !== 2 ||
      (!newlyVisible && !replacedWhileVisible)
    ) {
      return;
    }
    if (resolvePetRuntime(runtimeRef.current, Date.now()) !== "idle") return;
    if (transientTimerRef.current != null) {
      window.clearTimeout(transientTimerRef.current);
    }
    setActivity("waving");
    transientTimerRef.current = window.setTimeout(() => {
      transientTimerRef.current = null;
      setActivity(resolvePetRuntime(runtimeRef.current, Date.now()));
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
    const publish = () => {
      if (disposed) return;
      if (transientTimerRef.current != null)
        window.clearTimeout(transientTimerRef.current);
      transientTimerRef.current = null;
      const now = Date.now();
      setActivity(resolvePetRuntime(runtimeRef.current, now));
      const until = runtimeRef.current.transient?.until;
      if (until != null && until > now) {
        transientTimerRef.current = window.setTimeout(publish, until - now);
      }
    };
    const status = (payload: SessionStatusPayload) => {
      if (disposed) return;
      const next = applyPetSessionStatus(runtimeRef.current, payload);
      if (next === runtimeRef.current) return;
      runtimeRef.current = next;
      publish();
    };
    void (async () => {
      try {
        const stopStatus = await listen<SessionStatusPayload>(
          "session_status_changed",
          ({ payload }) => status(payload),
        );
        if (disposed) {
          stopStatus();
          return;
        }
        cleanups.push(stopStatus);
        const stopActivity = await listen<PetActivityPayload>(
          "desktop_pet_activity_changed",
          ({ payload }) => {
            if (disposed) return;
            const next = applyPetActivity(
              runtimeRef.current,
              payload,
              Date.now(),
            );
            if (next === runtimeRef.current) return;
            runtimeRef.current = next;
            publish();
          },
        );
        if (disposed) {
          stopActivity();
          return;
        }
        cleanups.push(stopActivity);
        const snapshot = await invoke<SessionStatusPayload[]>(
          "list_session_statuses",
        );
        if (!disposed) snapshot.forEach(status);
      } catch {
        // Keep the avatar available if the activity bridge is unavailable.
      }
    })();
    return () => {
      disposed = true;
      cleanups.forEach((stop) => stop());
      if (transientTimerRef.current != null)
        window.clearTimeout(transientTimerRef.current);
    };
  }, []);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    let previousX: number | null = null;
    void getCurrentWindow()
      .onMoved(({ payload }) => {
        if (disposed || reducedMotion) return;
        const dx = previousX == null ? 0 : payload.x - previousX;
        previousX = payload.x;
        if (dx !== 0) setDragState(dx < 0 ? "running-left" : "running-right");
        if (dragTimerRef.current != null)
          window.clearTimeout(dragTimerRef.current);
        dragTimerRef.current = window.setTimeout(() => setDragState(null), 180);
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else stop = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
      if (dragTimerRef.current != null)
        window.clearTimeout(dragTimerRef.current);
      setDragState(null);
    };
  }, [reducedMotion]);

  const petSrc = state.petPath ? resolveMediaSrc(state.petPath) : "";
  const renderedState: DesktopPetAnimationState = reducedMotion
    ? activity
    : (dragState ??
      (activity === "idle" && lookAngle != null ? "look" : activity));

  return (
    <main className="desktop-pet-surface">
      {error ? (
        <p className="desktop-pet-error" role="alert">
          {error}
        </p>
      ) : null}
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
          void getCurrentWindow()
            .startDragging()
            .catch(() => setDragState(null));
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
              void mutate("set_desktop_pet_enabled", { enabled: false }).catch(
                () => {},
              )
            }
          >
            <X size={14} />
          </button>
        </div>
      </div>
    </main>
  );
}
