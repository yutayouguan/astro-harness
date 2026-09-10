import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
import { usePetHitTesting } from "./usePetHitTesting";
import { motionDuration } from "../../lib/ui/petMotionClip";
import { DEFAULT_PET_PREFERENCES } from "../../lib/ui/petPreferences";
import { Menu } from "@tauri-apps/api/menu";
import type { PetScene } from "../../lib/ui/petScene";
import {
  canPlayPetLeisure,
  LEISURE_DURATION,
  type PetLeisure,
} from "../../lib/ui/desktopPetLeisure";

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
  const preferences = state.preferences ?? DEFAULT_PET_PREFERENCES;
  const [windowVisible, setWindowVisible] = useState(true);
  useEffect(() => {
    let disposed = false,
      received = false;
    let stop: (() => void) | undefined;
    void listen<boolean>("desktop-pet-visibility", ({ payload }) => {
      received = true;
      if (!disposed) setWindowVisible(payload);
    })
      .then(async (cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        stop = cleanup;
        const visible = await invoke<boolean>("get_desktop_pet_visible");
        if (!disposed && !received) setWindowVisible(visible);
      })
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  const [leisure, setLeisure] = useState<PetLeisure | null>(null);
  const leisureNext = useRef<PetLeisure>("kneading");
  const menuRef = useRef<Menu | null>(null);
  const menuOpening = useRef(false);
  const [menuError, setMenuError] = useState("");
  useEffect(
    () => () => {
      void menuRef.current?.close();
    },
    [],
  );
  async function showMenu() {
    if (menuOpening.current) return;
    menuOpening.current = true;
    setMenuError("");
    try {
      const scenes = await invoke<PetScene[]>("get_pet_scenes");
      const act = (command: string, args: Record<string, unknown>) => {
        void mutate(command, args).catch((e) => setMenuError(String(e)));
      };
      await menuRef.current?.close();
      menuRef.current = await Menu.new({
        items: [
          {
            id: "pet-open-main",
            text: "打开主窗口",
            action: () => {
              void invoke("open_desktop_pet_main").catch((e) =>
                setMenuError(String(e)),
              );
            },
          },
          {
            id: "pet-settings",
            text: "桌宠设置…",
            action: () => {
              void invoke("open_desktop_pet_main", { settings: true }).catch(
                (e) => setMenuError(String(e)),
              );
            },
          },
          {
            id: "pet-pause",
            text:
              state.spriteVersionNumber === 2
                ? state.animationPaused
                  ? "恢复动画"
                  : "暂停动画"
                : "静态形象（无动画帧）",
            enabled: state.spriteVersionNumber === 2,
            action: () =>
              act("edit_pet_scene", {
                request: { action: "pause", paused: !state.animationPaused },
              }),
          },
          {
            id: "pet-lock-position",
            text: preferences.positionLocked ? "解锁位置" : "锁定位置",
            action: () =>
              act("configure_desktop_pet_preferences", {
                patch: { positionLocked: !preferences.positionLocked },
              }),
          },
          {
            id: "pet-snap",
            text: preferences.snapToEdge ? "关闭贴边吸附" : "开启贴边吸附",
            action: () =>
              act("configure_desktop_pet_preferences", {
                patch: { snapToEdge: !preferences.snapToEdge },
              }),
          },
          {
            id: "pet-reset-position",
            text: "回到屏幕内",
            action: () => act("reset_desktop_pet_position", {}),
          },
          {
            id: "pet-quiet",
            text: preferences.quietMode ? "退出安静模式" : "安静模式",
            action: () =>
              act("configure_desktop_pet_preferences", {
                patch: { quietMode: !preferences.quietMode },
              }),
          },
          {
            id: "pet-presentation",
            text: "演示时暂时隐藏（托盘恢复）",
            action: () =>
              act("configure_desktop_pet_preferences", {
                patch: { presentationMode: true },
              }),
          },
          {
            id: "pet-hide",
            text: "暂时隐藏",
            action: () => act("set_desktop_pet_enabled", { enabled: false }),
          },
          ...(state.groomingPath ||
          state.motionClips?.grooming ||
          state.motionClips?.kneading
            ? [
                {
                  id: "pet-knead",
                  text: "踩奶",
                  enabled:
                    leisureAllowed &&
                    Boolean(state.motionClips?.kneading || state.groomingPath),
                  action: () => {
                    setLookAngle(null);
                    setLeisure("kneading");
                  },
                },
                {
                  id: "pet-groom",
                  text: "舔脚脚",
                  enabled:
                    leisureAllowed &&
                    Boolean(state.motionClips?.grooming || state.groomingPath),
                  action: () => {
                    setLookAngle(null);
                    setLeisure("grooming");
                  },
                },
              ]
            : []),
          {
            id: "pet-scenes",
            text: "切换场景",
            enabled: scenes.length > 0,
            items: scenes
              .sort((a, b) => Number(b.favorite) - Number(a.favorite))
              .slice(0, 20)
              .map((scene) => ({
                id: scene.id,
                text: (scene.favorite ? "★ " : "") + scene.name,
                action: () =>
                  act("apply_pet_scene", {
                    sceneId: scene.id,
                    mode: scene.wallpaperPath ? "all" : "pet",
                  }),
              })),
          },
        ],
      });
      await menuRef.current.popup(undefined, getCurrentWindow());
    } catch (e) {
      setMenuError(String(e));
    } finally {
      menuOpening.current = false;
    }
  }
  const [activity, setActivity] = useState<DesktopPetAnimationState>("idle");
  const [dragState, setDragState] = useState<
    "running-left" | "running-right" | null
  >(null);
  const [lookAngle, setLookAngle] = useState<number | null>(null);
  usePetHitTesting(state.enabled && windowVisible, dragState != null);
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
    }, 700);
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
    let movementGeneration = 0;
    const settle = (generation: number) => {
      void invoke<boolean>("settle_desktop_pet_position")
        .then((done) => {
          if (disposed || generation !== movementGeneration) return;
          if (!done) {
            dragTimerRef.current = window.setTimeout(
              () => settle(generation),
              180,
            );
            return;
          }
          dragTimerRef.current = null;
          setDragState(null);
        })
        .catch(() => {
          if (!disposed) setDragState(null);
        });
    };
    void getCurrentWindow()
      .onMoved(({ payload }) => {
        if (disposed) return;
        const dx = previousX == null ? 0 : payload.x - previousX;
        previousX = payload.x;
        if (dx !== 0 && !reducedMotion)
          setDragState(dx < 0 ? "running-left" : "running-right");
        if (dragTimerRef.current != null)
          window.clearTimeout(dragTimerRef.current);
        const generation = ++movementGeneration;
        dragTimerRef.current = window.setTimeout(() => settle(generation), 180);
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
  const leisureAllowed = canPlayPetLeisure({
    enabled: state.enabled && windowVisible,
    spriteVersionNumber: state.spriteVersionNumber,
    groomingPath: state.groomingPath,
    hasMotionClips: Boolean(
      state.motionClips?.grooming || state.motionClips?.kneading,
    ),
    paused: state.animationPaused,
    reducedMotion,
    activity,
    dragging: dragState != null,
    // Manual menu actions remain available in quiet mode; only auto-play stops.
    quietMode: false,
  });
  const leisureDuration =
    leisure && state.motionClips?.[leisure]
      ? motionDuration(state.motionClips[leisure])
      : leisure
        ? LEISURE_DURATION[leisure]
        : 0;
  useEffect(() => {
    if (!leisureAllowed) {
      setLeisure(null);
      return;
    }
    if (leisure) {
      const timer = window.setTimeout(() => setLeisure(null), leisureDuration);
      return () => window.clearTimeout(timer);
    }
    if (lookAngle != null || preferences.quietMode) return;
    const timer = window.setTimeout(() => {
      const available = (["kneading", "grooming"] as const).filter(
        (name) => state.motionClips?.[name] || state.groomingPath,
      );
      const next = available.includes(leisureNext.current)
        ? leisureNext.current
        : available[0];
      if (!next) return;
      leisureNext.current = next === "kneading" ? "grooming" : "kneading";
      setLeisure(next);
    }, preferences.activityIntervalSecs * 1000);
    return () => window.clearTimeout(timer);
  }, [
    leisureAllowed,
    leisure,
    lookAngle,
    state.petPath,
    state.groomingPath,
    leisureDuration,
    preferences.activityIntervalSecs,
    preferences.quietMode,
  ]);
  const playedLeisure = leisureAllowed ? leisure : null;
  const groomingSrc =
    playedLeisure === "grooming" ? resolveMediaSrc(state.groomingPath) : null;
  const renderedState: DesktopPetAnimationState =
    preferences.quietMode && !playedLeisure
      ? "idle"
      : playedLeisure === "kneading"
        ? "running"
        : reducedMotion
          ? activity
          : (dragState ??
            (activity === "idle" && lookAngle != null ? "look" : activity));

  return (
    <main className="desktop-pet-surface">
      {menuError && (
        <p className="desktop-pet-error" role="alert">
          {menuError}
        </p>
      )}
      {error ? (
        <p className="desktop-pet-error" role="alert">
          {error}
        </p>
      ) : null}
      <div
        className="desktop-pet-stage"
        role="group"
        data-position-locked={preferences.positionLocked}
        aria-label="桌面宠物，右键打开菜单；键盘操作请使用主窗口桌宠设置"
        onContextMenu={(event) => {
          event.preventDefault();
          void showMenu();
        }}
        onPointerMove={(event) => {
          if (leisure && !preferences.quietMode) setLeisure(null);
          if (
            reducedMotion ||
            preferences.quietMode ||
            state.animationPaused ||
            activity !== "idle" ||
            dragState
          )
            return;
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
          if (preferences.positionLocked) return;
          setLeisure(null);
          void invoke("begin_desktop_pet_drag").catch(() => setDragState(null));
        }}
        onDoubleClick={() => void invoke("open_desktop_pet_main")}
      >
        {petSrc && state.spriteVersionNumber === 2 ? (
          <DesktopPetCanvas
            src={petSrc}
            groomingSrc={resolveMediaSrc(state.groomingPath) || undefined}
            motionClips={state.motionClips}
            motionName={
              playedLeisure && state.motionClips?.[playedLeisure]
                ? playedLeisure
                : undefined
            }
            state={groomingSrc ? "idle" : renderedState}
            clip={groomingSrc ? "grooming" : undefined}
            lookAngle={lookAngle}
            reducedMotion={
              reducedMotion || state.animationPaused || !windowVisible
            }
            className="desktop-pet-character desktop-pet-character--canvas"
            label={state.displayName || "Animated desktop pet"}
          />
        ) : petSrc ? (
          <img
            crossOrigin="anonymous"
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
      </div>
    </main>
  );
}
