import { useEffect, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function useWindowChrome() {
  const [windowMaximized, setWindowMaximized] = useState(false);
  const zoomingRef = useRef(false);
  const titleDragTimerRef = useRef<number | null>(null);
  const titleLastClickRef = useRef({ time: 0, x: 0, y: 0 });

  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return;
    }
    let unlisten: (() => void) | undefined;
    try {
      const win = getCurrentWindow();
      void win
        .isMaximized()
        .then(setWindowMaximized)
        .catch(() => {});
      void win
        .onResized(() => {
          void win
            .isMaximized()
            .then(setWindowMaximized)
            .catch(() => {});
        })
        .then((fn) => {
          unlisten = fn;
        })
        .catch(() => {});
    } catch {
      // browser preview or Tauri internals not ready
    }
    return () => {
      unlisten?.();
    };
  }, []);

  const onTitleMouseDown = (e: ReactMouseEvent) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();

    const now = Date.now();
    const prev = titleLastClickRef.current;
    const isDouble =
      now - prev.time < 300 &&
      Math.abs(e.clientX - prev.x) < 5 &&
      Math.abs(e.clientY - prev.y) < 5;

    if (isDouble) {
      if (titleDragTimerRef.current != null) {
        window.clearTimeout(titleDragTimerRef.current);
        titleDragTimerRef.current = null;
      }
      titleLastClickRef.current = { time: 0, x: 0, y: 0 };
      if (zoomingRef.current) return;
      zoomingRef.current = true;
      const win = getCurrentWindow();
      void win.isMaximized().then((max) =>
        max ? win.unmaximize() : win.maximize()
      ).finally(() => {
        zoomingRef.current = false;
      });
      return;
    }

    titleLastClickRef.current = { time: now, x: e.clientX, y: e.clientY };
    if (titleDragTimerRef.current != null) {
      window.clearTimeout(titleDragTimerRef.current);
    }
    titleDragTimerRef.current = window.setTimeout(() => {
      titleDragTimerRef.current = null;
      void getCurrentWindow()
        .startDragging()
        .catch(() => {});
    }, 200);
  };

  const onTitleDoubleClick = (e: ReactMouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  return {
    windowMaximized,
    onTitleMouseDown,
    onTitleDoubleClick,
  };
}
