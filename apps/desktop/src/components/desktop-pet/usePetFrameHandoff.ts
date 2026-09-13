import { useLayoutEffect, type RefObject } from "react";

/** Keep the last *painted* frame while a different renderer decodes the next
 * action. A placeholder never acknowledges native locomotion readiness. */
export function usePetFrameHandoff(
  ref: RefObject<HTMLCanvasElement | null>,
  source: string,
  restore: (() => HTMLCanvasElement | null) | undefined,
  retain: ((canvas: HTMLCanvasElement) => void) | undefined,
) {
  useLayoutEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    const previous = restore?.();
    delete canvas.dataset.petFrameReady;
    context?.setTransform(1, 0, 0, 1, 0, 0);
    context?.clearRect(0, 0, canvas.width, canvas.height);
    if (previous && context) {
      canvas.width = previous.width;
      canvas.height = previous.height;
      canvas.style.aspectRatio = `${previous.width} / ${previous.height}`;
      context.drawImage(previous, 0, 0);
      canvas.dataset.petFrameReady = "true";
    }
    return () => {
      // Capture the source-bound callback from this effect, not the next pet's.
      if (canvas.dataset.petFrameReady === "true") retain?.(canvas);
    };
    // The callbacks are source-bound. Rebinding for unrelated prop updates would
    // reset playback or capture a new identity before its image has painted.
  }, [source]);
}
