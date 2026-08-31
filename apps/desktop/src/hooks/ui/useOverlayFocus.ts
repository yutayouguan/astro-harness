import { useEffect, useRef, type RefObject } from "react";

const FOCUSABLE_SELECTOR = [
  "a[href]",
  "button:not([disabled])",
  "input:not([disabled]):not([type='hidden'])",
  "select:not([disabled])",
  "textarea:not([disabled])",
  "[tabindex]:not([tabindex='-1'])",
].join(",");

export type UseOverlayFocusOptions = {
  open: boolean;
  onClose: () => void;
  containerRef: RefObject<HTMLElement>;
  initialFocusRef?: RefObject<HTMLElement>;
  closeOnEscape?: boolean;
  trapFocus?: boolean;
};

export function useOverlayFocus({
  open,
  onClose,
  containerRef,
  initialFocusRef,
  closeOnEscape = true,
  trapFocus = true,
}: UseOverlayFocusOptions) {
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;

  useEffect(() => {
    if (!open || typeof document === "undefined") return;
    const previousFocus = document.activeElement as HTMLElement | null;
    const frame = requestAnimationFrame(() => {
      const target =
        initialFocusRef?.current ??
        containerRef.current?.querySelector<HTMLElement>(FOCUSABLE_SELECTOR) ??
        containerRef.current;
      target?.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      const overlay = containerRef.current?.closest(".ui-overlay");
      const overlays = document.querySelectorAll(".ui-overlay");
      if (overlay && overlays[overlays.length - 1] !== overlay) return;

      if (event.key === "Escape" && closeOnEscape) {
        event.preventDefault();
        onCloseRef.current();
        return;
      }
      if (event.key !== "Tab" || !trapFocus) return;

      const container = containerRef.current;
      if (!container) return;
      const focusable = Array.from(
        container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR),
      ).filter((element) => !element.hidden);
      if (focusable.length === 0) {
        event.preventDefault();
        container.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last?.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first?.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown);
      previousFocus?.focus?.();
    };
  }, [closeOnEscape, containerRef, initialFocusRef, open, trapFocus]);
}
