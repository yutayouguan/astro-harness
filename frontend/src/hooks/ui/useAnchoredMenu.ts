/** 锚定菜单：测量宽度 + layoutAnchoredMenu + rAF 二次夹紧。 */
import {
  useLayoutEffect,
  useState,
  type RefObject,
} from "react";
import {
  layoutAnchoredMenu,
  type AnchoredMenuLayout,
} from "../../lib/ui/anchoredMenuLayout";
import {
  measurePopoverSize,
  resolveClipBounds,
  type ClampPopoverInput,
} from "../../lib/ui/clampPopover";

export type AnchoredMenuPos = AnchoredMenuLayout;

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
      const measured = menuRef.current
        ? measurePopoverSize(menuRef.current)
        : null;
      setPos(
        layoutAnchoredMenu({
          anchorRect: anchor.getBoundingClientRect(),
          bounds: resolveClipBounds(anchor),
          measured,
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
        }),
      );
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
