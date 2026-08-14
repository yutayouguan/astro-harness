import { useCallback, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import type { SidebarMenuAction } from "../../components/settings/SidebarContextMenu";

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
  const [sidebarCtx, setSidebarCtx] = useState<{ x: number; y: number } | null>(null);

  const hideTimerRef = useRef<number | null>(null);
  const sidebarCtxOpenRef = useRef(false);

  const openSidebar = useCallback(() => {
    if (hideTimerRef.current) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
    setSidebarOpen(true);
  }, []);

  const scheduleHideSidebar = useCallback(() => {
    if (sidebarPinned || sidebarCtxOpenRef.current) return;
    if (hideTimerRef.current) window.clearTimeout(hideTimerRef.current);
    hideTimerRef.current = window.setTimeout(() => {
      setSidebarOpen(false);
      hideTimerRef.current = null;
    }, 220);
  }, [sidebarPinned]);

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
    try { localStorage.setItem("astro.sidebarPinned", "0"); } catch { /* ignore */ }
  }, []);

  const pinSidebar = useCallback(() => {
    setSidebarPinned(true);
    setSidebarOpen(true);
    try { localStorage.setItem("astro.sidebarPinned", "1"); } catch { /* ignore */ }
  }, []);

  const sidebarVisible = sidebarOpen || sidebarPinned;
  const showSidebarLabels = sidebarVisible && (sidebarLabels || !sidebarPinned);

  return {
    sidebarPinned,
    sidebarOpen,
    sidebarLabels,
    sidebarCtx,
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
  };
}
