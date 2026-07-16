/** 媒体悬停工具条：下载 / 复制 */
import { useCallback, useState, type MouseEvent } from "react";
import { Check, Copy, Download } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  copyMedia,
  downloadMedia,
  type MediaActionKind,
} from "../../lib/mediaActions";

type Props = {
  path: string;
  kind: MediaActionKind;
  /** 紧凑：更小按钮 */
  compact?: boolean;
  className?: string;
};

export default function MediaToolbar({
  path,
  kind,
  compact,
  className,
}: Props) {
  const { t } = useI18n();
  const [busy, setBusy] = useState<"download" | "copy" | null>(null);
  const [copied, setCopied] = useState(false);
  const [err, setErr] = useState(false);

  const flashErr = useCallback(() => {
    setErr(true);
    window.setTimeout(() => setErr(false), 1600);
  }, []);

  const onDownload = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy) return;
      setBusy("download");
      try {
        await downloadMedia(path);
      } catch {
        flashErr();
      } finally {
        setBusy(null);
      }
    },
    [busy, path, flashErr],
  );

  const onCopy = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy) return;
      setBusy("copy");
      try {
        await copyMedia(path, kind);
        setCopied(true);
        window.setTimeout(() => setCopied(false), 1600);
      } catch {
        flashErr();
      } finally {
        setBusy(null);
      }
    },
    [busy, path, kind, flashErr],
  );

  const copyLabel = copied
    ? t("media.copied")
    : err
      ? t("media.actionFailed")
      : t("media.copy");
  const dlLabel = err ? t("media.actionFailed") : t("media.download");

  return (
    <div
      className={`media-toolbar ${compact ? "is-compact" : ""} ${className ?? ""}`.trim()}
      role="toolbar"
      aria-label={t("media.actions")}
    >
      <button
        type="button"
        className="media-toolbar-btn"
        onClick={(e) => void onDownload(e)}
        disabled={busy !== null}
        title={dlLabel}
        aria-label={dlLabel}
      >
        <Download size={compact ? 14 : 15} strokeWidth={2.1} aria-hidden />
      </button>
      <button
        type="button"
        className={`media-toolbar-btn ${copied ? "is-copied" : ""}`}
        onClick={(e) => void onCopy(e)}
        disabled={busy !== null}
        title={copyLabel}
        aria-label={copyLabel}
      >
        {copied ? (
          <Check size={compact ? 14 : 15} strokeWidth={2.4} aria-hidden />
        ) : (
          <Copy size={compact ? 14 : 15} strokeWidth={2.1} aria-hidden />
        )}
      </button>
    </div>
  );
}
