/** Pause freezes the displayed phase; a resumed clock never catches up hidden time. */
export function createPetPlaybackClock() {
  let elapsed = 0;
  let previous: number | null = null;
  let stopped = false;
  return {
    sample(now: number, paused: boolean) {
      if (!Number.isFinite(now)) return elapsed;
      if (previous !== null && !stopped && !paused)
        elapsed += Math.max(0, now - previous);
      previous = now;
      stopped = paused;
      return elapsed;
    },
    reset() {
      elapsed = 0;
      previous = null;
      stopped = false;
    },
  };
}
