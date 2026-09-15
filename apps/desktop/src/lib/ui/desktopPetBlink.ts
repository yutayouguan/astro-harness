/** Eye-only blink for the bundled Naitang artwork. No whole-head transforms. */
export const PET_BLINK_TIMING = {
  closing: 70,
  closed: 80,
  opening: 110,
} as const;
export const PET_BLINK_DURATION =
  PET_BLINK_TIMING.closing + PET_BLINK_TIMING.closed + PET_BLINK_TIMING.opening;
const smooth = (value: number) => value * value * (3 - 2 * value);

export function petBlinkClosure(elapsed: number): number {
  if (!Number.isFinite(elapsed) || elapsed < 0 || elapsed >= PET_BLINK_DURATION)
    return 0;
  const closedEnd = PET_BLINK_TIMING.closing + PET_BLINK_TIMING.closed;
  if (elapsed < PET_BLINK_TIMING.closing)
    return smooth(elapsed / PET_BLINK_TIMING.closing);
  if (elapsed < closedEnd) return 1;
  return 1 - smooth((elapsed - closedEnd) / PET_BLINK_TIMING.opening);
}

/** Random start-to-start intervals. Missed blinks never play in a catch-up burst. */
export function createPetBlinkTimeline(random: () => number = Math.random) {
  const gap = () => {
    const value = random();
    return (
      5000 +
      Math.max(0, Math.min(1, Number.isFinite(value) ? value : 0.5)) * 3000
    );
  };
  let next = gap();
  let previous = -1;
  return {
    sample(elapsed: number) {
      if (!Number.isFinite(elapsed) || elapsed < 0) return 0;
      if (elapsed < previous) next = gap();
      previous = elapsed;
      if (elapsed >= next + PET_BLINK_DURATION) {
        next += gap();
        if (elapsed >= next) next = elapsed + gap();
      }
      return petBlinkClosure(elapsed - next);
    },
  };
}

export function naitangBlinkProfile(path: string | null | undefined) {
  return path && /[/\\]builtin-naitang-v\d+[/\\]spritesheet\.webp$/.test(path)
    ? ("naitang" as const)
    : undefined;
}

// Eye apertures and corresponding closed-eye texture offsets in idle cells 0/1.
// These coordinates belong only to this bundled artwork, never arbitrary pets.
export const NAITANG_EYES = [
  { x: 92, y: 69, rx: 14, ry: 16, apertureX: 11, apertureY: 12, dx: -2, dy: 7 },
  { x: 123, y: 58, rx: 12, ry: 15, apertureX: 10, apertureY: 12, dx: 8, dy: 7 },
] as const;

export function composeNaitangBlink(
  neutral: Uint8ClampedArray,
  closed: Uint8ClampedArray,
  closure: number,
) {
  if (neutral.length !== 192 * 208 * 4 || closed.length !== neutral.length)
    throw new RangeError("Blink cells must be 192x208 RGBA");
  const result = new Uint8ClampedArray(neutral);
  const amount = Math.max(
    0,
    Math.min(1, Number.isFinite(closure) ? closure : 0),
  );
  if (!amount) return result;
  for (const eye of NAITANG_EYES) {
    for (let y = eye.y - eye.ry; y <= eye.y + eye.ry; y++) {
      for (let x = eye.x - eye.rx; x <= eye.x + eye.rx; x++) {
        const distance = Math.hypot((x - eye.x) / eye.rx, (y - eye.y) / eye.ry);
        if (distance >= 1) continue;
        const edge = Math.min(1, (1 - distance) / 0.12);
        const openHeight = eye.apertureY * (1 - amount);
        const aperture =
          openHeight > 0.1
            ? Math.hypot((x - eye.x) / eye.apertureX, (y - eye.y) / openHeight)
            : Infinity;
        const cover = Math.max(0, Math.min(1, (aperture - 0.9) / 0.15));
        const weight = edge * cover * Math.min(1, amount * 4);
        const at = (y * 192 + x) * 4;
        const from = ((y + eye.dy) * 192 + x + eye.dx) * 4;
        if (!closed[from + 3]) continue;
        for (let channel = 0; channel < 3; channel++) {
          result[at + channel] = Math.round(
            neutral[at + channel] * (1 - weight) +
              closed[from + channel] * weight,
          );
        }
        // Preserve the neutral silhouette/alpha exactly, including fine fur.
      }
    }
  }
  return result;
}

const frames = new WeakMap<HTMLImageElement, HTMLCanvasElement[] | null>();
export function naitangBlinkFrame(image: HTMLImageElement, closure: number) {
  let cached = frames.get(image);
  if (cached === undefined) {
    try {
      const source = document.createElement("canvas");
      source.width = 192;
      source.height = 208;
      const context = source.getContext("2d", { willReadFrequently: true });
      if (!context) {
        frames.set(image, null);
        return null;
      }
      context.drawImage(image, 0, 0, 192, 208, 0, 0, 192, 208);
      const neutral = context.getImageData(0, 0, 192, 208);
      context.clearRect(0, 0, 192, 208);
      context.drawImage(image, 192, 0, 192, 208, 0, 0, 192, 208);
      const closed = context.getImageData(0, 0, 192, 208);
      cached = Array.from({ length: 9 }, (_, i) => {
        const canvas = document.createElement("canvas");
        canvas.width = 192;
        canvas.height = 208;
        const frameContext = canvas.getContext("2d");
        if (!frameContext) throw new Error("Blink frame canvas unavailable");
        frameContext.putImageData(
          new ImageData(
            composeNaitangBlink(neutral.data, closed.data, i / 8),
            192,
            208,
          ),
          0,
          0,
        );
        return canvas;
      });
    } catch {
      cached = null;
    }
    frames.set(image, cached);
  }
  return cached?.[Math.round(Math.max(0, Math.min(1, closure)) * 8)] ?? null;
}
