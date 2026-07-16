/** 工作区批量操作条。 */
import { Copy, Scissors, Trash2, X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { FileMenuAction } from "../filespace/FileContextMenu";

/** 工作区批量操作条入参 */
type Props = {
  /** 已选文件数 */
  count: number;
  onAction: (action: FileMenuAction) => void;
  onClear: () => void;
};

const ICO = { size: 14, strokeWidth: 2.1 } as const;

export default function WorkspaceBatchBar({ count, onAction, onClear }: Props) {
  const { t } = useI18n();
  if (count < 1) return null;
  return (
    <div
      className="ws-batch-bar"
      role="toolbar"
      aria-label={t("workspace.batch.selected", { n: String(count) })}
    >
      <span className="ws-batch-count">
        {t("workspace.batch.selected", { n: String(count) })}
      </span>
      <button type="button" onClick={() => onAction("copyFile")}>
        <Copy {...ICO} aria-hidden />
        {t("workspace.menu.copy")}
      </button>
      <button type="button" onClick={() => onAction("cut")}>
        <Scissors {...ICO} aria-hidden />
        {t("workspace.menu.cut")}
      </button>
      <button
        type="button"
        className="is-danger"
        onClick={() => onAction("trash")}
      >
        <Trash2 {...ICO} aria-hidden />
        {t("workspace.menu.trash")}
      </button>
      <button type="button" className="ws-batch-clear" onClick={onClear}>
        <X {...ICO} aria-hidden />
        {t("workspace.batch.clear")}
      </button>
    </div>
  );
}
