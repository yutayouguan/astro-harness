/** 沙箱 HTML 预览：srcDoc + allow-scripts（无 same-origin / top-nav） */
import { useEffect, useMemo, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { useI18n } from "../../i18n/LocaleContext";
import { rewriteHtmlRelativeAssets } from "../../lib/media/htmlAssetRewrite";
import BrokenMedia from "./BrokenMedia";
import MediaToolbar from "./MediaToolbar";

type Props = {
  /** 本地文件路径（可选：用于系统打开 / 读内容） */
  path?: string | null;
  /** 内存 HTML；缺省且有 path 时 invoke read_file */
  source?: string | null;
  className?: string;
  compact?: boolean;
};

const SANDBOX = "allow-scripts";

/** 取文件所在目录（POSIX / Windows 均可） */
function dirnameOf(p: string): string {
  const norm = p.replace(/\\/g, "/");
  const idx = norm.lastIndexOf("/");
  return idx > 0 ? norm.slice(0, idx) : idx === 0 ? "/" : "";
}

/**
 * srcDoc 的基准 URL 是 about:srcdoc，相对资源（图片/CSS/JS）无从解析。
 * 注入指向文件所在目录的 <base href>（Tauri asset URL），让相对引用可加载。
 */
function withBaseHref(html: string, path: string): string {
  if (/<base\b/i.test(html)) return html;
  const dir = dirnameOf(path);
  if (!dir) return html;
  let baseUrl: string;
  try {
    baseUrl = convertFileSrc(dir);
  } catch {
    return html;
  }
  if (!baseUrl.endsWith("/")) baseUrl += "/";
  const tag = `<base href="${baseUrl}">`;
  if (/<head[^>]*>/i.test(html)) {
    return html.replace(/<head[^>]*>/i, (m) => `${m}${tag}`);
  }
  if (/<html[^>]*>/i.test(html)) {
    return html.replace(/<html[^>]*>/i, (m) => `${m}<head>${tag}</head>`);
  }
  return `${tag}${html}`;
}

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

  const srcDoc = useMemo(() => {
    if (doc == null || !path) return doc;
    const dir = dirnameOf(path);
    // 先把相对资源引用改写成绝对 asset URL（正确处理 ../），再补 <base> 作兜底
    const rewritten = dir
      ? rewriteHtmlRelativeAssets(doc, dir, convertFileSrc)
      : doc;
    return withBaseHref(rewritten, path);
  }, [doc, path]);

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
        <div className="html-preview-bar-actions">
          {path ? <MediaToolbar path={path} kind="html" compact /> : null}
        </div>
      </div>
      <iframe
        className="html-preview-frame"
        title={t("media.htmlPreview")}
        sandbox={SANDBOX}
        srcDoc={srcDoc ?? undefined}
        referrerPolicy="no-referrer"
      />
    </div>
  );
}
