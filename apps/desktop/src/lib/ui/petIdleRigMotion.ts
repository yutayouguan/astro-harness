/** Frame-rate independent, bounded local motion. No whole-animal scale. */
const ease = (x: number) => x * x * (3 - 2 * x);
export type RigGaze = {
  headX: number;
  headY: number;
  eyeX: number;
  eyeY: number;
};
export const restingGaze = (): RigGaze => ({
  headX: 0,
  headY: 0,
  eyeX: 0,
  eyeY: 0,
});
export function advanceRigGaze(
  previous: RigGaze,
  angle: number | null | undefined,
  dt: number,
): RigGaze {
  const valid = angle != null && Number.isFinite(angle);
  const radians = valid ? (angle * Math.PI) / 180 : 0;
  const x = valid ? Math.sin(radians) : 0,
    y = valid ? -Math.cos(radians) : 0;
  const follow = (from: number, to: number, tau: number) => {
    const next =
      from + (to - from) * (1 - Math.exp(-Math.max(0, Math.min(64, dt)) / tau));
    return Math.abs(next - to) < 0.005 ? to : next;
  };
  return {
    headX: follow(previous.headX, x * 1.6, 150),
    headY: follow(previous.headY, y * 0.8, 150),
    eyeX: follow(previous.eyeX, x * 1.1, 75),
    eyeY: follow(previous.eyeY, y * 0.7, 75),
  };
}
export function idleRigPose(elapsed: number, tailDegrees: number) {
  const time = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
  let phase = time % 24400;
  for (const gap of [5300, 7100, 6100, 5900]) {
    if (phase < gap) break;
    phase -= gap;
  }
  const blink =
    phase < 70
      ? ease(phase / 70)
      : phase < 150
        ? 1
        : phase < 260
          ? 1 - ease((phase - 150) / 110)
          : 0;
  const earPhase = (time + 3100) % 11300;
  const ear =
    earPhase < 500
      ? Math.sin((earPhase / 500) * Math.PI * 2) *
        Math.sin((earPhase / 500) * Math.PI) ** 2
      : 0;
  const tailPhase = (time + 3700) % 8700;
  const tail =
    tailPhase < 2900
      ? tailDegrees *
        Math.sin((tailPhase / 2900) * Math.PI * 4) *
        Math.sin((tailPhase / 2900) * Math.PI) ** 2
      : 0;
  return { blink: Math.round(blink * 8), ear, tail };
}
/** Long rests sleep until an actual local animation starts; pointer changes wake early. */
export function idleRigWakeMs(elapsed: number) {
  const time = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
  let blinkPhase = time % 24400,
    gap = 5300;
  for (const interval of [5300, 7100, 6100, 5900]) {
    gap = interval;
    if (blinkPhase < interval) break;
    blinkPhase -= interval;
  }
  const earPhase = (time + 3100) % 11300,
    tailPhase = (time + 3700) % 8700;
  if (blinkPhase < 260 || earPhase < 500 || tailPhase < 2900) return 32;
  return Math.max(
    16,
    Math.min(gap - blinkPhase, 11300 - earPhase, 8700 - tailPhase),
  );
}
export function fingerprintPixels(
  data: Uint8ClampedArray,
): Uint8Array<ArrayBuffer> {
  const result = new Uint8Array(data);
  for (let i = 0; i < result.length; i += 4)
    if (result[i + 3] !== 255) result.fill(0, i, i + 4);
  return result;
}
export function usesIdleRig(
  state: string,
  motionName?: string,
  clip?: string,
  externalElapsed?: number,
) {
  return (
    ["idle", "look"].includes(state) &&
    (!motionName || motionName === "idle") &&
    !clip &&
    externalElapsed === undefined
  );
}
