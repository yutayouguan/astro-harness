/**
 * 文件多选状态机：单击 / Cmd 切换 / Shift 范围选，供工作区与文件空间复用。
 */

/** 当前选中集合与 Shift 锚点 */
export type SelectionState = {
  selectedIds: Set<string>;
  anchorId: string | null;
};

/** 空选区 */
export function emptySelection(): SelectionState {
  return { selectedIds: new Set(), anchorId: null };
}

/** 单击：仅选中一项 */
export function applyClick(
  _state: SelectionState,
  id: string,
  _visibleIds: string[],
): SelectionState {
  return { selectedIds: new Set([id]), anchorId: id };
}

/** Cmd/Ctrl 单击：切换单项选中 */
export function applyToggle(
  state: SelectionState,
  id: string,
  _visibleIds: string[],
): SelectionState {
  const next = new Set(state.selectedIds);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  return { selectedIds: next, anchorId: id };
}

/** Shift 单击：从锚点到目标的连续范围 */
export function applyRange(
  state: SelectionState,
  id: string,
  visibleIds: string[],
): SelectionState {
  const anchor = state.anchorId ?? id;
  const i0 = visibleIds.indexOf(anchor);
  const i1 = visibleIds.indexOf(id);
  if (i0 < 0 || i1 < 0) {
    return applyClick(state, id, visibleIds);
  }
  const [lo, hi] = i0 <= i1 ? [i0, i1] : [i1, i0];
  return {
    selectedIds: new Set(visibleIds.slice(lo, hi + 1)),
    anchorId: state.anchorId ?? id,
  };
}

/** 右键菜单：若目标未选中则改为只选它 */
export function ensureMenuTarget(
  state: SelectionState,
  id: string,
): SelectionState {
  if (state.selectedIds.has(id)) return state;
  return { selectedIds: new Set([id]), anchorId: id };
}
