/** 浮层打开时按裁切盒钳制定位（resize / scroll 重算）。 */
import {
  useLayoutEffect,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import {
  clampPopover,
  measurePopoverSize,
  resolveClipBounds,
  type ClampPopoverInput,
} from "../lib/clampPopover";

type ClampOpts = Pick<
  ClampPopoverInput,
  "pad" | "gap" | "preferAlign" | "placement" | "maxHeightCap" | "minMaxHeight"
>;

type Opts = {
  open: boolean;
  anchorRef: RefObject<HTMLElement | null>;
  popoverRef: RefObject<HTMLElement | null>;
  /** 尺寸变化时触发重算（如编辑面板开关） */
  sizeKey?: unknown;
  mode?: "fixed" | "relative";
} & ClampOpts;

/**
 * 返回可直接赋给浮层的 style。
 * - fixed：视口 left/top（portal）
 * - relative：相对 anchor 的 offset（就地 absolute，并清掉 right）
 */
export function useClampPopover({
  open,
  anchorRef,
  popoverRef,
  sizeKey,
  mode = "fixed",
  pad,
  gap,
  preferAlign,
  placement,
  maxHeightCap,
  minMaxHeight,
}: Opts): CSSProperties | undefined {
  const [style, setStyle] = useState<CSSProperties | undefined>();

  useLayoutEffect(() => {
    if (!open || !anchorRef.current || !popoverRef.current) {
      setStyle(undefined);
      return;
    }
    const update = () => {
      const anchor = anchorRef.current;
      const popover = popoverRef.current;
      if (!anchor || !popover) return;
      const result = clampPopover({
        anchorRect: anchor.getBoundingClientRect(),
        popoverSize: measurePopoverSize(popover),
        bounds: resolveClipBounds(anchor),
        pad,
        gap,
        preferAlign,
        placement,
        maxHeightCap,
        minMaxHeight,
      });
      if (mode === "relative") {
        setStyle({
          left: result.offsetLeft,
          top: result.offsetTop,
          right: "auto",
          maxHeight: result.maxHeight,
        });
      } else {
        setStyle({
          left: result.left,
          top: result.top,
          maxHeight: result.maxHeight,
        });
      }
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
    mode,
    pad,
    gap,
    preferAlign,
    placement,
    maxHeightCap,
    minMaxHeight,
    anchorRef,
    popoverRef,
  ]);

  return style;
}
