/** 图 / 视频 / 音频内嵌预览（本地路径经 resolveMediaSrc）；悬停提供引用 / 放大 / 下载 / 复制 */
import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GeneratedMediaKind } from "../../lib/parseGeneratedMedia";
import { resolveMediaSrc } from "../../lib/resolveMediaSrc";
import BrokenMedia from "./BrokenMedia";
import HtmlPreview from "./HtmlPreview";
import MediaLightbox from "./MediaLightbox";
import MediaToolbar from "./MediaToolbar";

export type MediaPreviewProps = {
  kind: GeneratedMediaKind;
  /** 本地绝对路径或可加载 URL */
  path: string;
  /** HTML 源码（有则优先 srcDoc；否则尝试读 path） */
  htmlSource?: string | null;
  alt?: string;
  className?: string;
  compact?: boolean;
};

export default function MediaPreview({
  kind,
  path,
  htmlSource,
  alt,
  className,
  compact,
}: MediaPreviewProps) {
  const src = useMemo(() => resolveMediaSrc(path), [path]);
  const [broken, setBroken] = useState(!src && kind !== "html");
  const [lightboxOpen, setLightboxOpen] = useState(false);

  const openExternally = () => {
    void invoke("open_path_externally", { path }).catch(() => {});
  };

  if (kind === "html") {
    return (
      <HtmlPreview
        path={path}
        source={htmlSource}
        className={className}
        compact={compact}
      />
    );
  }

  if (broken || !src) {
    return (
      <BrokenMedia
        path={path}
        onOpenExternally={openExternally}
        className={className}
      />
    );
  }

  const wrap = `media-preview ${compact ? "is-compact" : ""} ${className ?? ""}`.trim();

  if (kind === "video") {
    return (
      <div className={wrap} data-kind="video">
        <video
          className="media-preview-video"
          src={src}
          controls
          playsInline
          preload="metadata"
          onError={() => setBroken(true)}
        />
        <MediaToolbar path={path} kind="video" compact={compact} />
      </div>
    );
  }

  if (kind === "audio") {
    return (
      <div className={wrap} data-kind="audio">
        <audio
          className="media-preview-audio"
          src={src}
          controls
          preload="metadata"
          onError={() => setBroken(true)}
        />
        <MediaToolbar path={path} kind="audio" compact={compact} />
      </div>
    );
  }

  return (
    <div className={wrap} data-kind="image">
      <button
        type="button"
        className="media-preview-image-btn"
        onClick={() => setLightboxOpen(true)}
        aria-label={alt?.trim() || undefined}
      >
        <img
          className="media-preview-image"
          src={src}
          alt={alt ?? ""}
          onError={() => setBroken(true)}
        />
      </button>
      <MediaToolbar path={path} kind="image" compact={compact} alt={alt} />
      {lightboxOpen ? (
        <MediaLightbox
          src={src}
          alt={alt}
          onClose={() => setLightboxOpen(false)}
        />
      ) : null}
    </div>
  );
}
