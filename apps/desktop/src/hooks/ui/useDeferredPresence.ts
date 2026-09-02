import { useLayoutEffect, useState } from "react";

/**
 * Keeps a surface mounted long enough to play its exit transition and delays
 * the visible state by one frame so newly mounted surfaces can animate in.
 */
export function useDeferredPresence(open: boolean, exitDurationMs = 300) {
  const [mounted, setMounted] = useState(open);
  const [visible, setVisible] = useState(false);

  useLayoutEffect(() => {
    let frame = 0;
    let timer = 0;

    if (open) {
      setMounted(true);
      frame = window.requestAnimationFrame(() => setVisible(true));
    } else {
      setVisible(false);
      timer = window.setTimeout(() => setMounted(false), exitDurationMs);
    }

    return () => {
      if (frame) window.cancelAnimationFrame(frame);
      if (timer) window.clearTimeout(timer);
    };
  }, [exitDurationMs, open]);

  return {
    mounted: open || mounted,
    visible: open && visible,
  };
}
