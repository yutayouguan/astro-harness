/** 模型选择器 flyout 视口钳制定位（相对 trigger 根节点）。 */

export type ModelPickerFlyoutPos = {
  left: number;
  right: "auto";
};

export type FlyoutBounds = {
  left: number;
  right: number;
};

/**
 * 将 flyout 放在 trigger 下方，默认右对齐；若超出 bounds 则平移，
 * 保证整块（含编辑面板）落在 [bounds.left + pad, bounds.right - pad] 内。
 */
export function clampModelPickerFlyout(
  rootRect: { left: number; width: number },
  flyoutWidth: number,
  bounds: FlyoutBounds,
  pad = 8,
): ModelPickerFlyoutPos {
  const innerLeft = bounds.left + pad;
  const innerRight = bounds.right - pad;
  const maxW = Math.max(0, innerRight - innerLeft);
  const width = Math.min(Math.max(flyoutWidth, 0), maxW);

  // 相对 root：默认右对齐（与 CSS right:0 一致）
  let left = rootRect.width - width;

  const absLeft = rootRect.left + left;
  if (absLeft < innerLeft) {
    left += innerLeft - absLeft;
  }

  const absRight = rootRect.left + left + width;
  if (absRight > innerRight) {
    left -= absRight - innerRight;
  }

  // 双侧都装不下时仍贴左安全边，避免再次右溢
  const finalLeft = rootRect.left + left;
  if (finalLeft < innerLeft) {
    left += innerLeft - finalLeft;
  }

  return { left, right: "auto" };
}

/** 优先取 content-pane 裁切盒，否则退回视口。 */
export function resolveFlyoutBounds(
  root: Element,
  viewportWidth: number,
): FlyoutBounds {
  const pane = root.closest(".content-pane");
  if (pane) {
    const r = pane.getBoundingClientRect();
    if (r.width > 0) return { left: r.left, right: r.right };
  }
  return { left: 0, right: viewportWidth };
}
