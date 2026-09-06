/** 生成中文件的实时预览面板（右栏 preview Tab）：HTML 实时渲染 / 代码实时高亮。 */
import { useCallback, useState } from "react";
import { FileCode2, Loader2 } from "lucide-react";
import { CopyMorphIcon } from "../icons/MorphIcon";
import { useI18n } from "../../i18n/LocaleContext";
import type { GeneratingPreview } from "../../hooks/chat/useGeneratingPreview";
import CodeFileCard from "../media/CodeFileCard";
import HtmlPreview from "../media/HtmlPreview";

type Props = {
  preview: GeneratingPreview | null;
};

export default function GeneratingPreviewPanel({ preview }: Props) {
  const { t } = useI18n();
  const [copied, setCopied] = useState(false);

  const onCopy = useCallback(async () => {
    if (!preview?.content) return;
    try {
      await navigator.clipboard.writeText(preview.content);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1600);
    } catch {
      // ignore
    }
  }, [preview?.content]);

  if (!preview) {
    return (
      <div className="gen-preview-empty">
        <FileCode2 size={26} strokeWidth={1.5} aria-hidden />
        <p>{t("chat.preview.empty")}</p>
      </div>
    );
  }

  const title = preview.filename ?? t("chat.preview.untitled");

  return (
    <div className="gen-preview">
      <div className="gen-preview-head">
        <span className="gen-preview-title" title={title}>
          {title}
        </span>
        <span
          className={`gen-preview-status ${preview.status}`}
          aria-label={
            preview.status === "streaming"
              ? t("chat.preview.streaming")
              : t("chat.preview.done")
          }
        >
          {preview.status === "streaming" ? (
            <>
              <Loader2
                size={12}
                strokeWidth={2.2}
                className="gen-preview-spin"
                aria-hidden
              />
              {t("chat.preview.streaming")}
            </>
          ) : (
            t("chat.preview.done")
          )}
        </span>
        <button
          type="button"
          className={`gen-preview-copy ${copied ? "is-copied" : ""}`}
          onClick={() => void onCopy()}
          disabled={!preview.content}
          title={copied ? t("chat.codeCopied") : t("chat.copyCode")}
          aria-label={copied ? t("chat.codeCopied") : t("chat.copyCode")}
        >
          <CopyMorphIcon copied={copied} size={14} aria-hidden />
        </button>
      </div>
      <div className="gen-preview-body">
        {preview.kind === "html" ? (
          <HtmlPreview source={preview.content} className="gen-preview-html" />
        ) : (
          <CodeFileCard
            source={preview.content}
            filename={preview.filename ?? undefined}
            className="gen-preview-code"
          />
        )}
      </div>
    </div>
  );
}
