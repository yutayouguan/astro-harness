/** 共享启发式浮层定位：贴边平移 / 上下翻转，优先 content-pane 裁切盒。 */

export type Bounds = {
  left: number;
  top: number;
  right: number;
  bottom: number;
};

export type RectLike = {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
};

export type ClampPopoverInput = {
  anchorRect: RectLike;
  popoverSize: { width: number; height: number };
  bounds: Bounds;
  /** 与裁切边的内边距，默认 8 */
  pad?: number;
  /** 与 trigger 的间距，默认 6 */
  gap?: number;
  /** 默认 end（右对齐）；start 为左对齐，右侧溢出时翻到 end */
  preferAlign?: "start" | "end";
  /** 默认 auto */
  placement?: "below" | "above" | "auto";
  /** 菜单最大高度软上限（如 260） */
  maxHeightCap?: number;
  /** maxHeight 下限，默认 0；SelectMenu 用 96 */
  minMaxHeight?: number;
};

export type ClampPopoverResult = {
  /** 视口坐标（fixed / portal） */
  left: number;
  top: number;
  placement: "below" | "above";
  maxHeight: number;
  /** 相对 anchor 左上角（就地 absolute） */
  offsetLeft: number;
  offsetTop: number;
};

const DEFAULT_PAD = 8;
const DEFAULT_GAP = 6;

/** 优先取 content-pane 裁切盒，否则视口。 */
export function resolveClipBounds(
  anchor: Element,
  viewport?: { width: number; height: number },
): Bounds {
  const pane = anchor.closest(".content-pane");
  if (pane) {
    const r = pane.getBoundingClientRect();
    if (r.width > 0 && r.height > 0) {
      return { left: r.left, top: r.top, right: r.right, bottom: r.bottom };
    }
  }
  const w =
    viewport?.width ??
    (typeof window !== "undefined" ? window.innerWidth : 0);
  const h =
    viewport?.height ??
    (typeof window !== "undefined" ? window.innerHeight : 0);
  return { left: 0, top: 0, right: w, bottom: h };
}

/** 不受父级 overflow:hidden 裁切低估影响。 */
export function measurePopoverSize(el: HTMLElement): {
  width: number;
  height: number;
} {
  return {
    width: Math.max(el.scrollWidth, el.offsetWidth),
    height: Math.max(el.scrollHeight, el.offsetHeight),
  };
}

/**
 * 将浮层放进 bounds；水平按 preferAlign，竖直 auto 时下方不够则上翻。
 * 返回的 top/left 始终是浮层盒在视口中的左上角。
 */
export function clampPopover(input: ClampPopoverInput): ClampPopoverResult {
  const pad = input.pad ?? DEFAULT_PAD;
  const gap = input.gap ?? DEFAULT_GAP;
  const preferAlign = input.preferAlign ?? "end";
  const placementPref = input.placement ?? "auto";
  const maxHeightCap = input.maxHeightCap ?? Number.POSITIVE_INFINITY;
  const minMaxHeight = input.minMaxHeight ?? 0;
  const { bounds, anchorRect } = input;

  const innerLeft = bounds.left + pad;
  const innerRight = bounds.right - pad;
  const innerTop = bounds.top + pad;
  const innerBottom = bounds.bottom - pad;

  const maxW = Math.max(0, innerRight - innerLeft);
  const width = Math.min(Math.max(input.popoverSize.width, 0), maxW);

  let left =
    preferAlign === "end" ? anchorRect.right - width : anchorRect.left;
  if (preferAlign === "start" && left + width > innerRight) {
    left = anchorRect.right - width;
  }
  if (preferAlign === "end" && left < innerLeft) {
    left = anchorRect.left;
  }
  if (left < innerLeft) left = innerLeft;
  if (left + width > innerRight) {
    left = Math.max(innerLeft, innerRight - width);
  }

  const spaceBelow = innerBottom - (anchorRect.bottom + gap);
  const spaceAbove = anchorRect.top - gap - innerTop;
  const rawH = Math.max(input.popoverSize.height, 0);
  const desiredH = Math.min(rawH, maxHeightCap);

  let placement: "below" | "above";
  if (placementPref === "below" || placementPref === "above") {
    placement = placementPref;
  } else {
    const need = Math.min(desiredH || 160, 160);
    placement =
      spaceBelow < need && spaceAbove > spaceBelow ? "above" : "below";
  }

  const avail = placement === "below" ? spaceBelow : spaceAbove;
  const maxHeight = Math.max(
    minMaxHeight,
    Math.min(maxHeightCap, Math.max(0, avail)),
  );
  const boxH = Math.min(desiredH || maxHeight, maxHeight);

  let top: number;
  if (placement === "below") {
    top = anchorRect.bottom + gap;
  } else {
    top = anchorRect.top - gap - boxH;
    if (top < innerTop) top = innerTop;
  }

  return {
    left,
    top,
    placement,
    maxHeight,
    offsetLeft: left - anchorRect.left,
    offsetTop: top - anchorRect.top,
  };
}
