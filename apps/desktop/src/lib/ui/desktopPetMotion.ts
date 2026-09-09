/** Stable timing helpers: keep sprite poses discrete, but damp the gaze target. */
export function shortestAngleDelta(from: number, to: number) {
  return ((((to - from + 540) % 360) + 360) % 360) - 180;
}

export function advancePetGaze(from: number, to: number, elapsedMs: number) {
  const delta = shortestAngleDelta(from, to);
  const dt = Math.max(0, Math.min(elapsedMs, 50));
  const step = delta * (1 - Math.exp(-dt / 70));
  return (from + Math.max(-dt * 0.6, Math.min(dt * 0.6, step)) + 360) % 360;
}

export function stablePetGazeFrame(angle: number, previous: number | null) {
  if (
    previous != null &&
    Math.abs(shortestAngleDelta(previous * 22.5, angle)) <= 14
  ) {
    return previous;
  }
  return Math.round((((angle % 360) + 360) % 360) / 22.5) % 16;
}

/** One closed-eye beat, separated by varied, deterministic resting intervals. */
export function idlePetFrame(elapsedMs: number) {
  const rests = [3200, 4600, 3800, 5200];
  const motion = [80, 100, 140, 160, 220];
  const motionDuration = motion.reduce((a, b) => a + b, 0);
  const total = rests.reduce((a, b) => a + b + motionDuration, 0);
  let cursor = ((elapsedMs % total) + total) % total;
  for (const rest of rests) {
    if (cursor < rest) return 0;
    cursor -= rest;
    for (let i = 0; i < motion.length; i++) {
      if (cursor < motion[i]) return i + 1;
      cursor -= motion[i];
    }
  }
  return 0;
}
