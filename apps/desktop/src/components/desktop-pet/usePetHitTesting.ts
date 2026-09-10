import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";
import { petPixelAt, startPetHitTesting } from "../../lib/ui/desktopPetHitTest";

// StrictMode cleanup/remounts share one write queue: late native completions
// cannot undo a newer loop's hit-test mode.
let writes: Promise<unknown> = Promise.resolve();
function setInteractive(interactive: boolean) {
  const next = writes.then(() =>
    invoke("set_desktop_pet_hit_test", { interactive }),
  );
  writes = next.catch(() => {});
  return next;
}

export function usePetHitTesting(enabled: boolean, dragging: boolean) {
  const draggingRef = useRef(dragging);
  draggingRef.current = dragging;
  useEffect(() => {
    if (!enabled || !("__TAURI_INTERNALS__" in window)) return;
    let pressedUntil = 0;
    const down = () => {
      pressedUntil = Date.now() + 600;
    };
    const up = () => {
      pressedUntil = 0;
    };
    window.addEventListener("pointerdown", down);
    window.addEventListener("pointerup", up);
    const staticBuffer = document.createElement("canvas");
    let staticSource = "";
    const stop = startPetHitTesting({
      probe: () => invoke<[number, number]>("desktop_pet_pointer"),
      setInteractive,
      schedule: (callback) => {
        const timer = window.setTimeout(callback, 40);
        return () => window.clearTimeout(timer);
      },
      hit: (x, y) => {
        if (draggingRef.current || Date.now() < pressedUntil) return true;
        const canvas = document.querySelector<HTMLCanvasElement>(
          ".desktop-pet-character--canvas",
        );
        if (canvas) {
          const pixel = petPixelAt(
            x,
            y,
            canvas.getBoundingClientRect(),
            canvas.width,
            canvas.height,
          );
          return Boolean(
            pixel &&
              canvas.getContext("2d")?.getImageData(pixel.x, pixel.y, 1, 1)
                .data[3],
          );
        }
        const image = document.querySelector<HTMLImageElement>(
          "img.desktop-pet-character",
        );
        if (!image?.complete || !image.naturalWidth) return false;
        if (staticSource !== image.currentSrc) {
          const ratio = Math.min(
            1,
            512 / Math.max(image.naturalWidth, image.naturalHeight),
          );
          staticBuffer.width = Math.max(
            1,
            Math.round(image.naturalWidth * ratio),
          );
          staticBuffer.height = Math.max(
            1,
            Math.round(image.naturalHeight * ratio),
          );
          staticBuffer
            .getContext("2d", { willReadFrequently: true })
            ?.drawImage(image, 0, 0, staticBuffer.width, staticBuffer.height);
          staticSource = image.currentSrc;
        }
        const rect = image.getBoundingClientRect();
        const scale = Math.min(
          rect.width / image.naturalWidth,
          rect.height / image.naturalHeight,
        );
        const width = image.naturalWidth * scale,
          height = image.naturalHeight * scale;
        const pixel = petPixelAt(
          x,
          y,
          {
            left: rect.left + (rect.width - width) / 2,
            top: rect.bottom - height,
            width,
            height,
          },
          staticBuffer.width,
          staticBuffer.height,
        );
        return Boolean(
          pixel &&
            staticBuffer.getContext("2d")?.getImageData(pixel.x, pixel.y, 1, 1)
              .data[3],
        );
      },
    });
    return () => {
      stop();
      window.removeEventListener("pointerdown", down);
      window.removeEventListener("pointerup", up);
    };
  }, [enabled]);
}
