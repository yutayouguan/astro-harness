export type PetRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export function petPixelAt(
  x: number,
  y: number,
  rect: PetRect,
  width: number,
  height: number,
) {
  if (
    rect.width <= 0 ||
    rect.height <= 0 ||
    width <= 0 ||
    height <= 0 ||
    x < rect.left ||
    y < rect.top ||
    x >= rect.left + rect.width ||
    y >= rect.top + rect.height
  )
    return null;
  return {
    x: Math.floor(((x - rect.left) * width) / rect.width),
    y: Math.floor(((y - rect.top) * height) / rect.height),
  };
}

/** Serialized probe loop; cleanup cannot be undone by a late native response. */
export function startPetHitTesting(options: {
  probe: () => Promise<readonly [number, number]>;
  hit: (x: number, y: number) => boolean;
  setInteractive: (value: boolean) => Promise<unknown>;
  schedule: (callback: () => void) => () => void;
}) {
  let disposed = false;
  let interactive: boolean | null = null;
  let cancel = () => {};
  const tick = async () => {
    try {
      const [x, y] = await options.probe();
      if (disposed) return;
      const next = options.hit(x, y);
      if (interactive !== next) {
        await options.setInteractive(next);
        interactive = next;
      }
    } catch {
      // Fail open: never leave an unrecoverable, unclickable companion.
      if (!disposed) await options.setInteractive(true).catch(() => {});
      interactive = null;
    } finally {
      if (!disposed) {
        cancel = options.schedule(() => void tick());
      }
    }
  };
  void tick();
  return () => {
    disposed = true;
    cancel();
    void options.setInteractive(true).catch(() => {});
  };
}
