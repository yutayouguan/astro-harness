export type DesktopPetAnimationState =
  | "idle"
  | "running-right"
  | "running-left"
  | "waving"
  | "jumping"
  | "failed"
  | "waiting"
  | "running"
  | "review"
  | "look";

export type DesktopPetFrame = {
  row: number;
  column: number;
};

type AnimationSpec = {
  row: number;
  durations: readonly number[];
};

export const DESKTOP_PET_CELL = { width: 192, height: 208 } as const;
export const DESKTOP_PET_ATLAS = { columns: 8, rows: 11 } as const;

export const DESKTOP_PET_ANIMATIONS: Record<
  Exclude<DesktopPetAnimationState, "look">,
  AnimationSpec
> = {
  idle: { row: 0, durations: [280, 110, 110, 140, 140, 320] },
  "running-right": {
    row: 1,
    durations: [120, 120, 120, 120, 120, 120, 120, 220],
  },
  "running-left": {
    row: 2,
    durations: [120, 120, 120, 120, 120, 120, 120, 220],
  },
  waving: { row: 3, durations: [140, 140, 140, 280] },
  jumping: { row: 4, durations: [140, 140, 140, 140, 280] },
  failed: {
    row: 5,
    durations: [140, 140, 140, 140, 140, 140, 140, 240],
  },
  waiting: { row: 6, durations: [150, 150, 150, 150, 150, 260] },
  running: { row: 7, durations: [120, 120, 120, 120, 120, 220] },
  review: { row: 8, durations: [150, 150, 150, 150, 150, 280] },
};

export function frameForElapsed(
  state: Exclude<DesktopPetAnimationState, "look">,
  elapsedMs: number,
  reducedMotion = false,
): DesktopPetFrame {
  const spec = DESKTOP_PET_ANIMATIONS[state];
  if (reducedMotion) return { row: spec.row, column: 0 };
  const total = spec.durations.reduce((sum, duration) => sum + duration, 0);
  let cursor = ((elapsedMs % total) + total) % total;
  for (let column = 0; column < spec.durations.length; column += 1) {
    const duration = spec.durations[column];
    if (cursor < duration) return { row: spec.row, column };
    cursor -= duration;
  }
  return { row: spec.row, column: spec.durations.length - 1 };
}

export function frameForLookAngle(angleDegrees: number): DesktopPetFrame {
  const normalized = ((angleDegrees % 360) + 360) % 360;
  const index = Math.round(normalized / 22.5) % 16;
  return index < 8 ? { row: 9, column: index } : { row: 10, column: index - 8 };
}

export type DesktopPetSessionStatus = {
  status: "idle" | "active" | "systemError";
  activeFlags: string[];
  updatedAt: number;
};

export function resolveDesktopPetActivity(
  statuses: Readonly<Record<string, DesktopPetSessionStatus>>,
): "idle" | "running" | "waiting" {
  const values = Object.values(statuses);
  if (
    values.some(
      (status) =>
        status.status === "active" &&
        status.activeFlags.some(
          (flag) =>
            flag === "waitingOnApproval" || flag === "waitingOnUserInput",
        ),
    )
  ) {
    return "waiting";
  }
  return values.some((status) => status.status === "active")
    ? "running"
    : "idle";
}
