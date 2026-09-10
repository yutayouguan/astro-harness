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

/** Blink with neutral -> closed eyes -> the same neutral pose.
 * The remaining idle-row cells contain head tilts, not blink in-betweens.
 */
export function idlePetFrame(elapsedMs: number) {
  const rests = [3200, 4600, 3800, 5200];
  const closedDuration = 80;
  // Keep the existing cadence while resting instead of playing head-tilt cells.
  const blinkDuration = closedDuration + 620;
  const total = rests.reduce((a, b) => a + b + blinkDuration, 0);
  let cursor = ((elapsedMs % total) + total) % total;
  for (const rest of rests) {
    if (cursor < rest) return 0;
    cursor -= rest;
    if (cursor < closedDuration) return 1;
    if (cursor < blinkDuration) return 0;
    cursor -= blinkDuration;
  }
  return 0;
}
