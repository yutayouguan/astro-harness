import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DesktopPetState } from "../../lib/ui/desktopPetState";
import { createPetMutationQueue } from "../../lib/ui/desktopPetState";
import {
  acceptRoamingFrame,
  roamingInterval,
  supportsPetRoaming,
  type PetRoamingFrame,
} from "../../lib/ui/petRoaming";

export function usePetRoaming(state: DesktopPetState, blocked: () => boolean) {
  const [frame, setFrame] = useState<PetRoamingFrame | null>(null);
  const suppressMoved = useRef(false);
  const blockedRef = useRef(blocked);
  blockedRef.current = blocked;
  const frameRef = useRef(frame);
  frameRef.current = frame;
  const binding = useRef(state);
  binding.current = state;
  const guardQueue = useRef(createPetMutationQueue());
  const sendGuard = (force?: boolean) =>
    guardQueue.current(() =>
      invoke<boolean>("report_desktop_pet_roaming_guard", {
        blocked: force ?? (document.hidden || blockedRef.current()),
      }),
    );
  const enabled =
    state.spriteVersionNumber === 3 &&
    !!state.preferences?.roamingEnabled &&
    supportsPetRoaming(state.motionClips);
  const interval = state.preferences?.activityIntervalSecs ?? 45;
  const signature = JSON.stringify([
    state.motionClips?.["running-left"],
    state.motionClips?.["running-right"],
  ]);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let disposed = false,
      stop: (() => void) | undefined;
    void listen<PetRoamingFrame>("desktop-pet-roaming", ({ payload }) => {
      if (disposed) return;
      if (
          (payload.active || payload.returning) &&
        (payload.petId !== binding.current.activePetId ||
          payload.revision !== binding.current.revision)
      )
        return;
      const accepted = acceptRoamingFrame(frameRef.current, payload);
      frameRef.current = accepted;
      if (accepted?.active) suppressMoved.current = true;
      setFrame(accepted);
    })
      .then((cleanup) => {
        if (disposed) cleanup();
        else stop = cleanup;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      stop?.();
    };
  }, []);
  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window) || !enabled) return;
    let disposed = false,
      timer = 0;
    const report = () =>
      void sendGuard()
        .then((suppress) => {
          if (!disposed) suppressMoved.current = suppress;
        })
        .catch(() => {});
    report();
    document.addEventListener("visibilitychange", report);
    // 隐藏时不再每 250ms 走一次 native：切换可见性的瞬间已经上报过 blocked，
    // 重复上报没有新信息，只会白占 IPC 与电量。
    const heartbeat = window.setInterval(() => {
      if (document.hidden) return;
      report();
    }, 250);
    const schedule = (delay: number) => {
      timer = window.setTimeout(() => void walk(), delay);
    };
    const walk = async () => {
      if (disposed) return;
      if (!blockedRef.current() && !frameRef.current?.active) {
        const right = Math.random() >= 0.5;
        const cycles = 2 + Math.floor(Math.random() * 3);
        for (const direction of [right, !right]) {
          if (disposed) break;
          try {
            const value = await invoke<PetRoamingFrame>(
              "start_desktop_pet_roaming",
              { petId: state.activePetId, right: direction, cycles },
            );
            if (!disposed) {
              const accepted = acceptRoamingFrame(frameRef.current, value);
              frameRef.current = accepted;
              suppressMoved.current = !!accepted?.active;
              setFrame(accepted);
            }
            break;
          } catch {
            /* Try the other edge once, then wait for the next interval. */
          }
        }
      }
      if (!disposed) schedule(roamingInterval(interval, Math.random()));
    };
    schedule(3000);
    return () => {
      disposed = true;
      clearTimeout(timer);
      clearInterval(heartbeat);
      document.removeEventListener("visibilitychange", report);
      void sendGuard(true).catch(() => {});
    };
  }, [enabled, interval, state.activePetId, state.petPath, signature]);
  return {
    frame:
      (frame?.active || frame?.returning) &&
      frame.petId === state.activePetId &&
      frame.revision === state.revision
        ? frame
        : null,
    suppressMoved,
    stop: () => {
      return enabled ? sendGuard(true).then(() => {}).catch(() => {}) : Promise.resolve();
    },
    beginDrag: () => {
      suppressMoved.current = false;
    },
    finishReturn: () => {
      const previous = frameRef.current;
      if (previous?.returning) {
        const next = { ...previous, returning: false, active: false, clip: null, clipName: null };
        frameRef.current = next; setFrame(next);
      }
    },
  };
}
