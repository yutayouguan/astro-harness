/** Same critical damping as the native frame driver; used by browser previews. */
export function advanceMorphSpring(
  value: number,
  velocity: number,
  target: number,
  dt: number,
) {
  const omega = 24,
    time = Math.max(0, Math.min(dt, 0.05)),
    delta = value - target,
    c = velocity + omega * delta,
    decay = Math.exp(-omega * time);
  return {
    value: target + (delta + c * time) * decay,
    velocity: (velocity - omega * c * time) * decay,
  };
}
export type PetTaskMorphFrame = {
  sequence: number;
  progress: number;
  left: boolean;
  width: number;
  height: number;
  contentWidth?: number;
  contentHeight?: number;
};
