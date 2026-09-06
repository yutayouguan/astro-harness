/** 图 / 视频 / 音频内嵌预览（本地路径经 resolveMediaSrc）；悬停提供引用 / 放大 / 下载 / 复制 */
import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GeneratedMediaKind } from "../../lib/media/parseGeneratedMedia";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import BrokenMedia from "./BrokenMedia";
import GlassAudioPlayer from "./GlassAudioPlayer";
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
  /** 是否显示下载/复制等工具条；卡片头已有操作时可关 */
  showToolbar?: boolean;
  /** 宿主可提供带自身边界校验的系统打开逻辑 */
  onOpenExternally?: () => void;
};

export default function MediaPreview({
  kind,
  path,
  htmlSource,
  alt,
  className,
  compact,
  showToolbar = true,
  onOpenExternally,
}: MediaPreviewProps) {
  const src = useMemo(() => resolveMediaSrc(path), [path]);
  // path/src 可能随后端或 mediaBaseDir 异步变为可加载 URL；勿把首屏失败粘成永久 Broken
  const [loadError, setLoadError] = useState(false);
  const [lightboxOpen, setLightboxOpen] = useState(false);

  useEffect(() => {
    setLoadError(false);
  }, [src]);

  const openExternally = () => {
    if (onOpenExternally) {
      onOpenExternally();
      return;
    }
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

  const broken = !src || loadError;
  if (broken) {
    return (
      <BrokenMedia
        path={path}
        onOpenExternally={openExternally}
        className={className}
      />
    );
  }

  const wrap =
    `media-preview ${compact ? "is-compact" : ""} ${className ?? ""}`.trim();

  if (kind === "video") {
    return (
      <div className={wrap} data-kind="video">
        <video
          className="media-preview-video"
          src={src}
          controls
          playsInline
          preload="metadata"
          onError={() => setLoadError(true)}
        />
        {showToolbar ? (
          <MediaToolbar path={path} kind="video" compact={compact} />
        ) : null}
      </div>
    );
  }

  if (kind === "audio") {
    return (
      <div className={wrap} data-kind="audio">
        {showToolbar ? (
          <div className="media-preview-audio-actions">
            <MediaToolbar
              path={path}
              kind="audio"
              compact={compact}
              className="is-inline"
            />
          </div>
        ) : null}
        <GlassAudioPlayer
          src={src}
          className="media-preview-audio"
          onError={() => setLoadError(true)}
        />
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
          onError={() => setLoadError(true)}
        />
      </button>
      {showToolbar ? (
        <MediaToolbar path={path} kind="image" compact={compact} alt={alt} />
      ) : null}
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
