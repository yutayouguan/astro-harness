import {
  resolveDesktopPetActivity,
  type DesktopPetAnimationState,
  type DesktopPetSessionStatus,
} from "./desktopPetAnimation.ts";

export type PetRuntime = {
  sessions: Record<string, DesktopPetSessionStatus>;
  activityTimestamps: Record<string, number>;
  transient: {
    state: "review" | "jumping" | "failed";
    sessionId: string;
    until: number;
  } | null;
};
export const emptyPetRuntime = (): PetRuntime => ({
  sessions: {},
  activityTimestamps: {},
  transient: null,
});
type Payload = {
  sessionId?: unknown;
  status?: unknown;
  activeFlags?: unknown;
  state?: unknown;
  tsMs?: unknown;
};

export function applyPetSessionStatus(
  runtime: PetRuntime,
  payload: Payload,
): PetRuntime {
  if (
    typeof payload.sessionId !== "string" ||
    !payload.sessionId.trim() ||
    typeof payload.tsMs !== "number" ||
    !Number.isFinite(payload.tsMs) ||
    !["idle", "active", "systemError"].includes(String(payload.status))
  )
    return runtime;
  const id = payload.sessionId;
  if ((runtime.sessions[id]?.updatedAt ?? -1) >= payload.tsMs) return runtime;
  const session: DesktopPetSessionStatus = {
    status: payload.status as DesktopPetSessionStatus["status"],
    activeFlags: Array.isArray(payload.activeFlags)
      ? payload.activeFlags.filter(
          (flag): flag is string => typeof flag === "string",
        )
      : [],
    updatedAt: payload.tsMs,
  };
  return {
    ...runtime,
    sessions: { ...runtime.sessions, [id]: session },
    transient:
      runtime.transient?.sessionId === id &&
      payload.tsMs > (runtime.activityTimestamps[id] ?? -1)
        ? null
        : runtime.transient,
  };
}

export function applyPetActivity(
  runtime: PetRuntime,
  payload: Payload,
  now: number,
): PetRuntime {
  if (
    typeof payload.sessionId !== "string" ||
    !payload.sessionId.trim() ||
    typeof payload.tsMs !== "number" ||
    !Number.isFinite(payload.tsMs)
  )
    return runtime;
  const id = payload.sessionId;
  if (
    payload.tsMs <= (runtime.activityTimestamps[id] ?? -1) ||
    payload.tsMs < (runtime.sessions[id]?.updatedAt ?? -1)
  )
    return runtime;
  if (
    !["review", "jumping", "failed", "waiting", "idle"].includes(
      String(payload.state),
    )
  )
    return runtime;
  const next = {
    ...runtime,
    activityTimestamps: { ...runtime.activityTimestamps, [id]: payload.tsMs },
  };
  if (payload.state === "review" && runtime.sessions[id]?.status !== "active")
    return next;
  if (
    payload.state === "review" ||
    payload.state === "jumping" ||
    payload.state === "failed"
  ) {
    return {
      ...next,
      transient: {
        state: payload.state,
        sessionId: id,
        until:
          now +
          (payload.state === "review"
            ? 1500
            : payload.state === "jumping"
              ? 980
              : 2200),
      },
    };
  }
  return {
    ...next,
    transient: runtime.transient?.sessionId === id ? null : runtime.transient,
  };
}

export function resolvePetRuntime(
  runtime: PetRuntime,
  now: number,
): DesktopPetAnimationState {
  const aggregate = resolveDesktopPetActivity(runtime.sessions);
  if (aggregate === "waiting") return "waiting";
  const transient = runtime.transient;
  if (transient && now < transient.until) {
    if (transient.state === "jumping")
      return aggregate === "idle" ? "jumping" : aggregate;
    return transient.state;
  }
  return aggregate;
}
