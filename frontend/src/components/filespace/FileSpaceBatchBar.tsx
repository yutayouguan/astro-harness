/** 文件空间批量操作条。 */
import {
  ClipboardCopy,
  Copy,
  MessageSquare,
  MessageSquarePlus,
  Trash2,
  X,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { FileMenuAction } from "./FileContextMenu";

/** 文件空间批量操作条入参 */
type Props = {
  /** 已选文件数 */
  count: number;
  onAction: (action: FileMenuAction) => void;
  onClear: () => void;
  disableCopyFile?: boolean;
  disableAttach?: boolean;
  disableTrash?: boolean;
};

const ICO = { size: 14, strokeWidth: 2.1 } as const;

export default function FileSpaceBatchBar({
  count,
  onAction,
  onClear,
  disableCopyFile,
  disableAttach,
  disableTrash,
}: Props) {
  const { t } = useI18n();
  if (count < 1) return null;
  return (
    <div
      className="fs-batch-bar"
      role="toolbar"
      aria-label={t("filespace.batch.selected", { n: String(count) })}
    >
      <span className="fs-batch-count">
        {t("filespace.batch.selected", { n: String(count) })}
      </span>
      <button type="button" onClick={() => onAction("copyPath")}>
        <ClipboardCopy {...ICO} aria-hidden />
        {t("filespace.menu.copyPath")}
      </button>
      <button
        type="button"
        disabled={disableCopyFile}
        onClick={() => onAction("copyFile")}
      >
        <Copy {...ICO} aria-hidden />
        {t("filespace.menu.copyFile")}
      </button>
      <button
        type="button"
        disabled={disableAttach}
        onClick={() => onAction("attachNew")}
      >
        <MessageSquarePlus {...ICO} aria-hidden />
        {t("filespace.menu.attachNew")}
      </button>
      <button
        type="button"
        disabled={disableAttach}
        onClick={() => onAction("attachCurrent")}
      >
        <MessageSquare {...ICO} aria-hidden />
        {t("filespace.menu.attachCurrent")}
      </button>
      <button
        type="button"
        className="is-danger"
        disabled={disableTrash}
        onClick={() => onAction("trash")}
      >
        <Trash2 {...ICO} aria-hidden />
        {t("filespace.menu.trash")}
      </button>
      <button type="button" className="fs-batch-clear" onClick={onClear}>
        <X {...ICO} aria-hidden />
        {t("filespace.batch.clear")}
      </button>
    </div>
  );
}
