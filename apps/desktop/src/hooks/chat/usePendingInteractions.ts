import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  acceptInteractions,
  EMPTY_INTERACTIONS,
  type InteractionState,
  type PendingInteraction,
} from "../../lib/chat/pendingInteractions";

let state = EMPTY_INTERACTIONS;
const subscribers = new Set<() => void>();
let started = false;
const inflight = new Set<string>();
export function receiveInteractionState(next: InteractionState) {
  state = acceptInteractions(state, next);
  for (const update of subscribers) update();
}
const accept = receiveInteractionState;
function start() {
  if (started || !("__TAURI_INTERNALS__" in window)) return;
  started = true;
  void listen<InteractionState>("pending-interactions-changed", ({ payload }) =>
    accept(payload),
  )
    .then(() => {
      const beforeFetch = state;
      void invoke<InteractionState>("get_pending_interactions")
        .then(accept)
        .catch(() => {
          if (state === beforeFetch) accept({ ...state, connected: false });
        });
    })
    .catch(() => {
      accept({ ...state, connected: false });
      started = false;
      window.setTimeout(start, 1000);
    });
}
export function usePendingInteractions() {
  const value = useSyncExternalStore(
    (notify) => {
      subscribers.add(notify);
      return () => {
        subscribers.delete(notify);
      };
    },
    () => state,
    () => EMPTY_INTERACTIONS,
  );
  useEffect(start, []);
  return value;
}
export async function respondInteraction(
  request: PendingInteraction,
  action: string,
  payload: Record<string, unknown>,
  confirmedPersistent = false,
) {
  if (inflight.has(request.key)) throw new Error("此请求正在提交");
  if (
    !state.connected ||
    !state.snapshot.requests.some((r) => r.key === request.key)
  )
    throw new Error("请求已失效或连接中断");
  inflight.add(request.key);
  try {
    accept(
      await invoke<InteractionState>("respond_pending_interaction", {
        request: {
          key: request.key,
          sessionId: request.sessionId,
          turnId: request.turnId,
          action,
          payload,
          confirmedPersistent,
        },
      }),
    );
  } finally {
    inflight.delete(request.key);
  }
}
export function openInteractionSession(request: {
  sessionId: string;
  key?: string;
}) {
  return invoke("open_pet_task_session", {
    sessionId: request.sessionId,
    requestKey: request.key ?? null,
  });
}
