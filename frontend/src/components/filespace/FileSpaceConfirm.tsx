/** 文件空间危险操作确认（AppDialog 薄封装，兼容旧 Props API）。 */
import AppDialog from "../ui/AppDialog";
import { useI18n } from "../../i18n/LocaleContext";

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
    <AppDialog
      open
      title={title}
      message={message}
      variant="danger"
      confirmLabel={confirmLabel}
      cancelLabel={t("filespace.confirm.cancel")}
      onCancel={onCancel}
      onConfirm={onConfirm}
    />
  );
}
