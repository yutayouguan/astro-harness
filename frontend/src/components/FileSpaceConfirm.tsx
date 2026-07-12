/** 文件空间危险操作确认。 */
import { useI18n } from "../i18n/LocaleContext";

/** 文件空间危险操作确认入参 */
type Props = {
  title: string;
  message: string;
  confirmLabel: string;
  onCancel: () => void;
  onConfirm: () => void;
};

export default function FileSpaceConfirm({
  title,
  message,
  confirmLabel,
  onCancel,
  onConfirm,
}: Props) {
  const { t } = useI18n();
  return (
    <div className="fs-confirm-backdrop" role="presentation" onClick={onCancel}>
      <div
        className="fs-confirm"
        role="dialog"
        aria-modal
        aria-labelledby="fs-confirm-title"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="fs-confirm-title">{title}</h3>
        <p>{message}</p>
        <div className="fs-confirm-actions">
          <button type="button" className="fs-confirm-cancel" onClick={onCancel}>
            {t("filespace.confirm.cancel")}
          </button>
          <button type="button" className="fs-confirm-ok" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
