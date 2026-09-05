import { useCallback, useLayoutEffect, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import type { SidebarMenuAction } from "../../components/settings/SidebarContextMenu";
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SIDEBAR_WIDTH_KEY,
  clampSidebarWidth,
  maxSidebarWidth,
  parseStoredSidebarWidth,
  shouldUseCompactSidebar,
} from "../../lib/ui/sidebarWidth";

const RESIZE_KEYBOARD_STEP = 16;
const RESIZE_KEYBOARD_LARGE_STEP = 48;

export function useSidebar() {
  const [sidebarPinned, setSidebarPinned] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarPinned") !== "0";
    } catch {
      return true;
    }
  });
  const [sidebarOpen, setSidebarOpen] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarPinned") !== "0";
    } catch {
      return true;
    }
  });
  const [sidebarLabels, setSidebarLabels] = useState(() => {
    try {
      return localStorage.getItem("astro.sidebarLabels") === "1";
    } catch {
      return false;
    }
  });
  const [sidebarCtx, setSidebarCtx] = useState<{ x: number; y: number } | null>(
    null,
  );
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    try {
      return parseStoredSidebarWidth(localStorage.getItem(SIDEBAR_WIDTH_KEY));
    } catch {
      return SIDEBAR_DEFAULT_WIDTH;
    }
  });
  const [sidebarMaxWidth, setSidebarMaxWidth] = useState(SIDEBAR_DEFAULT_WIDTH);
  const [sidebarCompact, setSidebarCompact] = useState(false);
  const [sidebarResizing, setSidebarResizing] = useState(false);

  const hideTimerRef = useRef<number | null>(null);
  const sidebarCtxOpenRef = useRef(false);
  const sidebarRef = useRef<HTMLElement>(null);
  const sidebarWidthRef = useRef(sidebarWidth);
  const sidebarResizingRef = useRef(false);
  const resizeRef = useRef<{
    pointerId: number;
    startX: number;
    startWidth: number;
  } | null>(null);

  sidebarWidthRef.current = sidebarWidth;

  const containerWidth = useCallback(() => {
    const width =
      sidebarRef.current?.parentElement?.getBoundingClientRect().width ?? 0;
    return width > 0 ? width : Number.POSITIVE_INFINITY;
  }, []);

  const updateSidebarWidth = useCallback(
    (nextWidth: number, persist = false) => {
      const next = clampSidebarWidth(nextWidth, containerWidth());
      sidebarWidthRef.current = next;
      setSidebarWidth(next);
      if (persist) {
        try {
          localStorage.setItem(SIDEBAR_WIDTH_KEY, String(next));
        } catch {
          // Storage can be unavailable in private or locked-down webviews.
        }
      }
    },
    [containerWidth],
  );

  useLayoutEffect(() => {
    const container = sidebarRef.current?.parentElement;
    if (!container) return;

    const syncBounds = () => {
      const width = container.getBoundingClientRect().width;
      if (width <= 0) return;
      setSidebarCompact(shouldUseCompactSidebar(width));
      setSidebarMaxWidth(maxSidebarWidth(width));
      const next = clampSidebarWidth(sidebarWidthRef.current, width);
      sidebarWidthRef.current = next;
      setSidebarWidth(next);
    };

    syncBounds();
    const observer =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(syncBounds)
        : null;
    observer?.observe(container);
    window.addEventListener("resize", syncBounds);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", syncBounds);
    };
  }, []);

  const openSidebar = useCallback(() => {
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setSidebarOpen(true);
  }, []);

  const scheduleHideSidebar = useCallback(() => {
    if (
      sidebarPinned ||
      sidebarCtxOpenRef.current ||
      sidebarResizingRef.current
    )
      return;
    if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => {
      setSidebarOpen(false);
      hideTimerRef.current = null;
    }, 220);
  }, [sidebarPinned]);

  const finishSidebarResize = useCallback((pointerId: number) => {
    if (resizeRef.current?.pointerId !== pointerId) return;
    resizeRef.current = null;
    sidebarResizingRef.current = false;
    setSidebarResizing(false);
    try {
      localStorage.setItem(SIDEBAR_WIDTH_KEY, String(sidebarWidthRef.current));
    } catch {
      // Storage can be unavailable in private or locked-down webviews.
    }
  }, []);

  const onSidebarResizePointerDown = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      event.stopPropagation();
      if (hideTimerRef.current) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
      setSidebarOpen(true);
      resizeRef.current = {
        pointerId: event.pointerId,
        startX: event.clientX,
        startWidth:
          sidebarRef.current?.getBoundingClientRect().width ??
          sidebarWidthRef.current,
      };
      sidebarResizingRef.current = true;
      setSidebarResizing(true);
      event.currentTarget.setPointerCapture(event.pointerId);
    },
    [],
  );

  const onSidebarResizePointerMove = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      const drag = resizeRef.current;
      if (!drag || drag.pointerId !== event.pointerId) return;
      updateSidebarWidth(drag.startWidth + event.clientX - drag.startX);
    },
    [updateSidebarWidth],
  );

  const onSidebarResizePointerUp = useCallback(
    (event: ReactPointerEvent<HTMLButtonElement>) => {
      finishSidebarResize(event.pointerId);
      if (event.currentTarget.hasPointerCapture(event.pointerId)) {
        event.currentTarget.releasePointerCapture(event.pointerId);
      }
    },
    [finishSidebarResize],
  );

  const onSidebarResizeKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLButtonElement>) => {
      const step = event.shiftKey
        ? RESIZE_KEYBOARD_LARGE_STEP
        : RESIZE_KEYBOARD_STEP;
      let nextWidth: number | null = null;
      if (event.key === "ArrowRight")
        nextWidth = sidebarWidthRef.current + step;
      else if (event.key === "ArrowLeft")
        nextWidth = sidebarWidthRef.current - step;
      else if (event.key === "Home") nextWidth = SIDEBAR_MIN_WIDTH;
      else if (event.key === "End") nextWidth = sidebarMaxWidth;
      if (nextWidth == null) return;
      event.preventDefault();
      event.stopPropagation();
      updateSidebarWidth(nextWidth, true);
    },
    [sidebarMaxWidth, updateSidebarWidth],
  );

  const resetSidebarWidth = useCallback(() => {
    updateSidebarWidth(SIDEBAR_DEFAULT_WIDTH, true);
  }, [updateSidebarWidth]);

  const toggleSidebar = useCallback(() => {
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setSidebarPinned((pinned) => {
      const next = !pinned;
      setSidebarOpen(next);
      try {
        localStorage.setItem("astro.sidebarPinned", next ? "1" : "0");
      } catch {
        // ignore quota / private mode
      }
      return next;
    });
  }, []);

  const toggleSidebarLabels = useCallback(() => {
    setSidebarLabels((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("astro.sidebarLabels", next ? "1" : "0");
      } catch {
        // ignore
      }
      return next;
    });
  }, []);

  const openSidebarContextMenu = useCallback((e: ReactMouseEvent) => {
    e.preventDefault();
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    sidebarCtxOpenRef.current = true;
    setSidebarOpen(true);
    setSidebarCtx({ x: e.clientX, y: e.clientY });
  }, []);

  const closeSidebarContextMenu = useCallback(() => {
    sidebarCtxOpenRef.current = false;
    setSidebarCtx(null);
    if (!sidebarPinned) {
      if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = window.setTimeout(() => {
        setSidebarOpen(false);
        hideTimerRef.current = null;
      }, 220);
    }
  }, [sidebarPinned]);

  const onSidebarContextAction = useCallback((action: SidebarMenuAction) => {
    if (action === "toggleLabels") {
      setSidebarLabels((prev) => {
        const next = !prev;
        try {
          localStorage.setItem("astro.sidebarLabels", next ? "1" : "0");
        } catch {
          // ignore
        }
        return next;
      });
    } else {
      if (hideTimerRef.current) {
        window.clearTimeout(hideTimerRef.current);
        hideTimerRef.current = null;
      }
      setSidebarPinned((pinned) => {
        const next = !pinned;
        setSidebarOpen(next);
        try {
          localStorage.setItem("astro.sidebarPinned", next ? "1" : "0");
        } catch {
          // ignore quota / private mode
        }
        return next;
      });
    }
    sidebarCtxOpenRef.current = false;
    setSidebarCtx(null);
  }, []);

  const collapseSidebar = useCallback(() => {
    setSidebarPinned(false);
    setSidebarOpen(false);
    try {
      localStorage.setItem("astro.sidebarPinned", "0");
    } catch {
      /* ignore */
    }
  }, []);

  const pinSidebar = useCallback(() => {
    setSidebarPinned(true);
    setSidebarOpen(true);
    try {
      localStorage.setItem("astro.sidebarPinned", "1");
    } catch {
      /* ignore */
    }
  }, []);

  const sidebarVisible = sidebarOpen || sidebarPinned;
  const showSidebarLabels =
    sidebarVisible && !sidebarCompact && (sidebarLabels || !sidebarPinned);

  return {
    sidebarPinned,
    sidebarOpen,
    sidebarLabels,
    sidebarCtx,
    sidebarRef,
    sidebarWidth,
    sidebarCompact,
    sidebarMinWidth: Math.min(SIDEBAR_MIN_WIDTH, sidebarMaxWidth),
    sidebarMaxWidth,
    sidebarResizing,
    sidebarVisible,
    showSidebarLabels,
    openSidebar,
    scheduleHideSidebar,
    toggleSidebar,
    toggleSidebarLabels,
    openSidebarContextMenu,
    closeSidebarContextMenu,
    onSidebarContextAction,
    collapseSidebar,
    pinSidebar,
    onSidebarResizePointerDown,
    onSidebarResizePointerMove,
    onSidebarResizePointerUp,
    finishSidebarResize,
    onSidebarResizeKeyDown,
    resetSidebarWidth,
  };
}
