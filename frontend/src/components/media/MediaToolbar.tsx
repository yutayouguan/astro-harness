/** 媒体悬停工具条：下载 / 复制；下载成功以 Toast 提示保存路径 */
import { useCallback, useState, type MouseEvent } from "react";
import { Check, Copy, Download } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useTransientToast } from "../../hooks/useTransientToast";
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
  const { showToast, toastHost } = useTransientToast();
  const [busy, setBusy] = useState<"download" | "copy" | null>(null);
  const [copied, setCopied] = useState(false);

  const onDownload = useCallback(
    async (e: MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (busy) return;
      setBusy("download");
      try {
        const saved = await downloadMedia(path);
        showToast(t("media.downloadSuccess", { path: saved }), {
          tone: "success",
          durationMs: 8000,
        });
      } catch (err) {
        showToast(
          `${t("media.actionFailed")}${err ? `：${String(err)}` : ""}`,
          { error: true },
        );
      } finally {
        setBusy(null);
      }
    },
    [busy, path, showToast, t],
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
        showToast(t("media.copied"), { tone: "success" });
        window.setTimeout(() => setCopied(false), 1600);
      } catch (err) {
        showToast(
          `${t("media.actionFailed")}${err ? `：${String(err)}` : ""}`,
          { error: true },
        );
      } finally {
        setBusy(null);
      }
    },
    [busy, path, kind, showToast, t],
  );

  const copyLabel = copied ? t("media.copied") : t("media.copy");
  const dlLabel = t("media.download");

  return (
    <>
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
      {toastHost}
    </>
  );
}
