import { useCallback, useLayoutEffect, useRef, useState } from "react";

let topLayer = 0;
const MAX_CSS_LAYER = 2_147_483_647;

export function nextOverlayLayer(
  previous: number,
  observed: readonly number[],
): number {
  const usable = observed.filter(
    (value) => Number.isFinite(value) && value >= 0 && value < MAX_CSS_LAYER,
  );
  return Math.max(0, previous, ...usable) + 1;
}

function documentLayers(): number[] {
  if (typeof document === "undefined") return [];
  const layers: number[] = [];
  for (const element of document.querySelectorAll<HTMLElement>("body *")) {
    const style = window.getComputedStyle(element);
    if (style.position === "static") continue;
    const value = Number.parseInt(style.zIndex, 10);
    if (Number.isFinite(value)) layers.push(value);
  }
  return layers;
}

/** Return the next layer above every currently known app surface. */
export function claimOverlayLayer(scanDocument = false): number {
  topLayer = nextOverlayLayer(topLayer, scanDocument ? documentLayers() : []);
  return topLayer;
}

/**
 * Assigns an overlay a runtime layer. Opening a surface places it on top;
 * pointer interaction can promote an older non-modal surface again.
 */
export function useDynamicOverlayLayer(open: boolean) {
  const [layer, setLayer] = useState<number | undefined>();
  const wasOpen = useRef(false);

  useLayoutEffect(() => {
    if (open && !wasOpen.current) setLayer(claimOverlayLayer(true));
    wasOpen.current = open;
  }, [open]);

  const bringToFront = useCallback(() => {
    if (open && layer !== topLayer) setLayer(claimOverlayLayer());
  }, [layer, open]);

  return { layer, bringToFront };
}
