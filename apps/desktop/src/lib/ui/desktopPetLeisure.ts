export type PetLeisure = "kneading" | "grooming";
export const GROOMING_DURATIONS = [260, 160, 160, 160, 160, 300] as const;
export const LEISURE_DURATION = { kneading: 1640, grooming: 2400 } as const;

export function groomingFrame(elapsed: number, reducedMotion = false) {
  if (reducedMotion || !Number.isFinite(elapsed)) return { row: 0, column: 0 };
  const total = GROOMING_DURATIONS.reduce<number>((sum, n) => sum + n, 0);
  let cursor = ((elapsed % total) + total) % total;
  for (let column = 0; column < GROOMING_DURATIONS.length; column++) {
    if (cursor < GROOMING_DURATIONS[column]) return { row: 0, column };
    cursor -= GROOMING_DURATIONS[column];
  }
  return { row: 0, column: 0 };
}

export function canPlayPetLeisure(input: {
  enabled: boolean;
  spriteVersionNumber: number | null;
  groomingPath: string | null;
  paused: boolean;
  reducedMotion: boolean;
  activity: string;
  dragging: boolean;
}) {
  return (
    input.enabled &&
    input.spriteVersionNumber === 2 &&
    Boolean(input.groomingPath) &&
    !input.paused &&
    !input.reducedMotion &&
    input.activity === "idle" &&
    !input.dragging
  );
}
