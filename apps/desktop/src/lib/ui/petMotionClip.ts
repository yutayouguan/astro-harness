export type PetMotionClip = {
  path: string;
  frameWidth: number;
  frameHeight: number;
  columns: number;
  durationsMs: number[];
  loopStart: number;
  loopEnd: number;
  loopRepeats: number;
  neutralBookends?: boolean;
};
export type PetMotionClips = Record<string, PetMotionClip>;

export function motionUsesNeutralFrame(
  clip: PetMotionClip,
  row: number,
  column: number,
) {
  const index = row * clip.columns + column;
  return Boolean(
    clip.neutralBookends &&
      (index === 0 || index === clip.durationsMs.length - 1),
  );
}

export function motionSequence(clip: PetMotionClip) {
  const sequence = Array.from({ length: clip.loopStart }, (_, i) => i);
  for (let repeat = 0; repeat < clip.loopRepeats; repeat++) {
    for (let i = clip.loopStart; i < clip.loopEnd; i++) sequence.push(i);
  }
  for (let i = clip.loopEnd; i < clip.durationsMs.length; i++) sequence.push(i);
  return sequence;
}
export function motionDuration(clip: PetMotionClip) {
  return motionSequence(clip).reduce((sum, i) => sum + clip.durationsMs[i], 0);
}
export function motionFrame(
  clip: PetMotionClip,
  elapsed: number,
  reducedMotion = false,
  repeat = false,
) {
  const sequence = motionSequence(clip);
  const duration = motionDuration(clip);
  let cursor = Math.max(0, Number.isFinite(elapsed) ? elapsed : 0);
  if (repeat && duration > 0) cursor %= duration;
  let index = sequence[sequence.length - 1];
  if (reducedMotion) index = 0;
  else
    for (const candidate of sequence) {
      if (cursor < clip.durationsMs[candidate]) {
        index = candidate;
        break;
      }
      cursor -= clip.durationsMs[candidate];
    }
  return {
    row: Math.floor(index / clip.columns),
    column: index % clip.columns,
    done: !repeat && elapsed >= duration,
  };
}
