import { useEffect, useState } from "react";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { createWallpaperLivePreview } from "../../lib/ui/wallpaperLivePreview";

export function useWallpaperLivePreview() {
  const [preview] = useState(() =>
    createWallpaperLivePreview((path) => {
      if (typeof document === "undefined") return null;
      const src = resolveMediaSrc(path);
      if (!src) return null;
      const expected = new URL(src, document.baseURI).href;
      const layer = [
        ...document.querySelectorAll<HTMLElement>(".shell-wallpaper-layer"),
      ].find(
        (element) =>
          element.querySelector<HTMLImageElement>("img")?.src === expected,
      );
      if (!layer) return null;
      return {
        style: layer.style,
        matches: () =>
          layer.isConnected &&
          layer.querySelector<HTMLImageElement>("img")?.src === expected,
        observe: (changed) => {
          const observer = new MutationObserver(changed);
          observer.observe(layer, {
            attributes: true,
            childList: true,
            subtree: true,
            attributeFilter: ["src", "style"],
          });
          return () => observer.disconnect();
        },
      };
    }),
  );
  useEffect(() => () => preview.cancel(), [preview]);
  return preview;
}
