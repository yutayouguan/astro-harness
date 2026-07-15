/** 沙箱 HTML 预览：srcDoc + allow-scripts（无 same-origin / top-nav） */
import { useEffect, useState } from "react";
import { ExternalLink } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n/LocaleContext";
import BrokenMedia from "./BrokenMedia";

type Props = {
  /** 本地文件路径（可选：用于系统打开 / 读内容） */
  path?: string | null;
  /** 内存 HTML；缺省且有 path 时 invoke read_file */
  source?: string | null;
  className?: string;
  compact?: boolean;
};

const SANDBOX = "allow-scripts";

export default function HtmlPreview({
  path,
  source,
  className,
  compact,
}: Props) {
  const { t } = useI18n();
  const [doc, setDoc] = useState<string | null>(source ?? null);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (source != null) {
      setDoc(source);
      setError(false);
      return;
    }
    if (!path) {
      setDoc(null);
      setError(true);
      return;
    }
    let cancelled = false;
    setError(false);
    void invoke<string>("read_file", { path })
      .then((text) => {
        if (!cancelled) setDoc(text);
      })
      .catch(() => {
        if (!cancelled) {
          setDoc(null);
          setError(true);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [path, source]);

  const openExternally = () => {
    if (!path) return;
    void invoke("open_path_externally", { path }).catch(() => {});
  };

  if (error || doc == null) {
    return (
      <BrokenMedia
        path={path}
        onOpenExternally={path ? openExternally : undefined}
        className={className}
      />
    );
  }

  return (
    <div
      className={`html-preview ${compact ? "is-compact" : ""} ${className ?? ""}`.trim()}
    >
      <div className="html-preview-bar">
        <span className="html-preview-label">{t("media.htmlPreview")}</span>
        {path ? (
          <button
            type="button"
            className="html-preview-open"
            onClick={openExternally}
          >
            <ExternalLink size={13} strokeWidth={2.1} aria-hidden />
            {t("workspace.openExternally")}
          </button>
        ) : null}
      </div>
      <iframe
        className="html-preview-frame"
        title={t("media.htmlPreview")}
        sandbox={SANDBOX}
        srcDoc={doc}
        referrerPolicy="no-referrer"
      />
    </div>
  );
}
