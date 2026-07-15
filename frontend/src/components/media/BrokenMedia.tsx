/** 媒体加载失败占位 */
import { ImageOff } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";

type Props = {
  path?: string | null;
  onOpenExternally?: () => void;
  className?: string;
};

export default function BrokenMedia({
  path,
  onOpenExternally,
  className,
}: Props) {
  const { t } = useI18n();
  return (
    <div className={`media-broken ${className ?? ""}`.trim()}>
      <ImageOff size={22} strokeWidth={1.8} aria-hidden />
      <p>{t("media.loadError")}</p>
      {path ? <p className="media-broken-path">{path}</p> : null}
      {onOpenExternally ? (
        <button
          type="button"
          className="media-broken-open"
          onClick={onOpenExternally}
        >
          {t("workspace.openExternally")}
        </button>
      ) : null}
    </div>
  );
}
