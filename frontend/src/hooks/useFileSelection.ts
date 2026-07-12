/**
 * 文件列表多选 Hook：单击 / Cmd / Shift，供工作区与文件空间使用。
 */

import { useCallback, useState } from "react";
import {
  applyClick,
  applyRange,
  applyToggle,
  emptySelection,
  ensureMenuTarget,
  type SelectionState,
} from "../lib/fileSelection";

/**
 * @param visibleIds 当前可见行的稳定 id 列表（Shift 范围依赖顺序）
 */
export function useFileSelection(visibleIds: string[]) {
  const [state, setState] = useState<SelectionState>(() => emptySelection());

  const onItemClick = useCallback(
    (id: string, e: { metaKey?: boolean; ctrlKey?: boolean; shiftKey?: boolean }) => {
      setState((prev) => {
        if (e.shiftKey) return applyRange(prev, id, visibleIds);
        if (e.metaKey || e.ctrlKey) return applyToggle(prev, id, visibleIds);
        return applyClick(prev, id, visibleIds);
      });
    },
    [visibleIds],
  );

  const onCheckboxToggle = useCallback(
    (id: string) => {
      setState((prev) => applyToggle(prev, id, visibleIds));
    },
    [visibleIds],
  );

  const prepareMenu = useCallback((id: string) => {
    setState((prev) => ensureMenuTarget(prev, id));
  }, []);

  const clear = useCallback(() => setState(emptySelection()), []);

  return {
    selectedIds: state.selectedIds,
    anchorId: state.anchorId,
    onItemClick,
    onCheckboxToggle,
    prepareMenu,
    clear,
    setOnly: (id: string) => setState(applyClick(emptySelection(), id, visibleIds)),
  };
}
