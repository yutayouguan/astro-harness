/** 锚定菜单：测量宽度 + clampPopover + rAF 二次夹紧。 */
import {
  useLayoutEffect,
  useState,
  type RefObject,
} from "react";
import {
  clampPopover,
  measurePopoverSize,
  resolveClipBounds,
  type ClampPopoverInput,
} from "../lib/clampPopover";

export type AnchoredMenuPos = {
  top: number;
  left: number;
  width: number;
  openUp: boolean;
  maxHeight: number;
};

type Opts = {
  open: boolean;
  anchorRef: RefObject<HTMLElement | null>;
  menuRef: RefObject<HTMLElement | null>;
  sizeKey?: unknown;
  /** 菜单最小宽；未传 fixedWidth 时与触发器宽度取 max */
  minWidth?: number;
  /** 菜单最大宽，默认 280 */
  maxWidth?: number;
  /** 固定宽度（Cron 等）时忽略测量 min/max */
  fixedWidth?: number;
  /** 高度软上限，默认 260；再与 bounds 高 * ratio 取 min */
  maxHeightCap?: number;
  /** 相对裁切盒高度比例，默认 0.42 */
  maxHeightRatio?: number;
  minMaxHeight?: number;
} & Pick<ClampPopoverInput, "pad" | "gap" | "preferAlign" | "placement">;

/**
 * Portal / fixed 菜单定位结果；未打开时为 null。
 * 首帧用触发器宽估算，下一帧按菜单真实 scrollWidth 再夹一次。
 */
export function useAnchoredMenu({
  open,
  anchorRef,
  menuRef,
  sizeKey,
  minWidth = 140,
  maxWidth = 280,
  fixedWidth,
  maxHeightCap = 260,
  maxHeightRatio = 0.42,
  minMaxHeight = 96,
  pad,
  gap,
  preferAlign = "start",
  placement = "auto",
}: Opts): AnchoredMenuPos | null {
  const [pos, setPos] = useState<AnchoredMenuPos | null>(null);

  useLayoutEffect(() => {
    if (!open || !anchorRef.current) {
      setPos(null);
      return;
    }

    const update = () => {
      const anchor = anchorRef.current;
      if (!anchor) return;
      const rect = anchor.getBoundingClientRect();
      const bounds = resolveClipBounds(anchor);
      const boundsW = Math.max(0, bounds.right - bounds.left - (pad ?? 8) * 2);
      const heightCap = Math.min(
        maxHeightCap,
        Math.max(0, (bounds.bottom - bounds.top) * maxHeightRatio),
      );

      let width: number;
      let height: number;
      if (fixedWidth != null) {
        width = Math.min(fixedWidth, boundsW);
        height = menuRef.current
          ? measurePopoverSize(menuRef.current).height
          : 160;
      } else {
        const floor = Math.max(rect.width, minWidth);
        const ceil = Math.min(maxWidth, boundsW);
        const measured = menuRef.current
          ? measurePopoverSize(menuRef.current)
          : { width: floor, height: 160 };
        width = Math.min(Math.max(measured.width, floor), ceil);
        height = measured.height;
      }

      const clamped = clampPopover({
        anchorRect: rect,
        popoverSize: { width, height },
        bounds,
        pad,
        gap,
        preferAlign,
        placement,
        maxHeightCap: heightCap,
        minMaxHeight,
      });

      setPos({
        top: clamped.top,
        left: clamped.left,
        width,
        openUp: clamped.placement === "above",
        maxHeight: clamped.maxHeight,
      });
    };

    update();
    const raf = requestAnimationFrame(update);
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [
    open,
    sizeKey,
    minWidth,
    maxWidth,
    fixedWidth,
    maxHeightCap,
    maxHeightRatio,
    minMaxHeight,
    pad,
    gap,
    preferAlign,
    placement,
    anchorRef,
    menuRef,
  ]);

  return pos;
}
