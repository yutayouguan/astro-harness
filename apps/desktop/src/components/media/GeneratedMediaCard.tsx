/** 多媒体生成结果卡片：标题行含操作，下方仅播放/预览。 */
import {
  Clapperboard,
  Code2,
  FileCode2,
  Image as ImageIcon,
  Music2,
} from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { GeneratedMediaKind } from "../../lib/media/parseGeneratedMedia";
import { displayTitleFromMediaPath } from "../../lib/media/displayTitleFromMediaPath";
import CodeFileCard from "./CodeFileCard";
import MediaPreview from "./MediaPreview";
import MediaToolbar from "./MediaToolbar";

export type GeneratedMediaCardProps = {
  kind: GeneratedMediaKind;
  /** 已解析为可预览的绝对路径或 URL */
  path: string;
  label?: string | null;
  className?: string;
  compact?: boolean;
};

function KindGlyph({ kind }: { kind: GeneratedMediaKind }) {
  const props = { size: 16, strokeWidth: 2.1, "aria-hidden": true as const };
  switch (kind) {
    case "audio":
      return <Music2 {...props} />;
    case "video":
      return <Clapperboard {...props} />;
    case "html":
      return <FileCode2 {...props} />;
    case "code":
      return <Code2 {...props} />;
    default:
      return <ImageIcon {...props} />;
  }
}

export default function GeneratedMediaCard({
  kind,
  path,
  label,
  className,
  compact,
}: GeneratedMediaCardProps) {
  const { t } = useI18n();
  const title = displayTitleFromMediaPath(path);
  const kindLabel =
    kind === "audio"
      ? t("media.kind.audio")
      : kind === "video"
        ? t("media.kind.video")
        : kind === "html"
          ? t("media.kind.html")
          : kind === "code"
            ? t("media.kind.code")
            : t("media.kind.image");
  const customLabel = label?.trim();

  return (
    <article
      className={`gen-media-card ${compact ? "is-compact" : ""} ${className ?? ""}`.trim()}
      data-kind={kind}
    >
      <header className="gen-media-card-head">
        <span
          className="gen-media-card-glyph"
          role="img"
          aria-label={kindLabel}
        >
          <KindGlyph kind={kind} />
        </span>
        <div className="gen-media-card-titles">
          <h3 className="gen-media-card-title">{title}</h3>
          {customLabel ? (
            <p className="gen-media-card-caption">{customLabel}</p>
          ) : null}
        </div>
        <MediaToolbar
          path={path}
          kind={kind}
          compact
          className="is-inline gen-media-card-actions"
          alt={title}
        />
      </header>
      <div className="gen-media-card-body">
        {kind === "code" ? (
          <CodeFileCard
            path={path}
            compact={compact}
            className="gen-media-card-preview"
          />
        ) : (
          <MediaPreview
            kind={kind}
            path={path}
            alt={title}
            compact={compact}
            showToolbar={false}
            className="gen-media-card-preview"
          />
        )}
      </div>
    </article>
  );
}
