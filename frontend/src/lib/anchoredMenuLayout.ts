/** 锚定菜单定位纯函数（供 useAnchoredMenu 与单测共用）。 */
import {
  clampPopover,
  type Bounds,
  type ClampPopoverInput,
  type RectLike,
} from "./clampPopover.ts";

export type AnchoredMenuLayout = {
  top: number;
  left: number;
  width: number;
  /** content-pane 内可用最大宽（fixedWidth / maxWidth / bounds 取 min） */
  widthCap: number;
  openUp: boolean;
  maxHeight: number;
};

export type AnchoredMenuLayoutInput = {
  anchorRect: Pick<
    RectLike,
    "left" | "top" | "right" | "bottom" | "width" | "height"
  >;
  bounds: Bounds;
  /** 已测菜单尺寸；缺省时用触发器宽估宽、估高 160 */
  measured?: { width: number; height: number } | null;
  minWidth?: number;
  maxWidth?: number;
  fixedWidth?: number;
  maxHeightCap?: number;
  maxHeightRatio?: number;
  minMaxHeight?: number;
} & Pick<ClampPopoverInput, "pad" | "gap" | "preferAlign" | "placement">;

/** 计算 portal 锚定菜单的 left/top/width/maxHeight。 */
export function layoutAnchoredMenu(
  input: AnchoredMenuLayoutInput,
): AnchoredMenuLayout {
  const pad = input.pad ?? 8;
  const minWidth = input.minWidth ?? 140;
  const maxWidth = input.maxWidth ?? 280;
  const maxHeightCap = input.maxHeightCap ?? 260;
  const maxHeightRatio = input.maxHeightRatio ?? 0.42;
  const minMaxHeight = input.minMaxHeight ?? 96;
  const bounds = input.bounds;
  const rect = input.anchorRect;
  const boundsW = Math.max(0, bounds.right - bounds.left - pad * 2);
  const heightCap = Math.min(
    maxHeightCap,
    Math.max(0, (bounds.bottom - bounds.top) * maxHeightRatio),
  );

  let width: number;
  let height: number;
  let widthCap: number;
  if (input.fixedWidth != null) {
    widthCap = Math.min(input.fixedWidth, boundsW);
    width = widthCap;
    height = input.measured?.height ?? 160;
  } else {
    widthCap = Math.min(maxWidth, boundsW);
    const floor = Math.max(rect.width, minWidth);
    const measured = input.measured ?? { width: floor, height: 160 };
    width = Math.min(Math.max(measured.width, floor), widthCap);
    height = measured.height;
  }

  const clamped = clampPopover({
    anchorRect: rect,
    popoverSize: { width, height },
    bounds,
    pad: input.pad,
    gap: input.gap,
    preferAlign: input.preferAlign,
    placement: input.placement,
    maxHeightCap: heightCap,
    minMaxHeight,
  });

  return {
    top: clamped.top,
    left: clamped.left,
    width,
    widthCap,
    openUp: clamped.placement === "above",
    maxHeight: clamped.maxHeight,
  };
}
