/** 媒体加载失败占位 */
import { FileQuestion, ImageOff } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";

type Props = {
  path?: string | null;
  reason?: "not-found" | "load-error";
  onOpenExternally?: () => void;
  className?: string;
};

export default function BrokenMedia({
  path,
  reason,
  onOpenExternally,
  className,
}: Props) {
  const { t } = useI18n();
  const isNotFound = reason === "not-found";
  return (
    <div className={`media-broken ${isNotFound ? "is-not-found" : ""} ${className ?? ""}`.trim()}>
      {isNotFound ? (
        <FileQuestion size={22} strokeWidth={1.8} aria-hidden />
      ) : (
        <ImageOff size={22} strokeWidth={1.8} aria-hidden />
      )}
      <p>{isNotFound ? t("media.fileNotFound") : t("media.loadError")}</p>
      {path ? <p className="media-broken-path">{path}</p> : null}
      {onOpenExternally && !isNotFound ? (
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
