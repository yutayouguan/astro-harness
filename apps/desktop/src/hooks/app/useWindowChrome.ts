import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

export function useWindowChrome() {
  const [windowMaximized, setWindowMaximized] = useState(false);

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

  return {
    windowMaximized,
  };
}
