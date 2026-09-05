/** 文件预览/编辑内容区（工作空间与文件空间共用）。
 *  只负责按类型渲染内容：text/markdown/html 走编辑器或预览，image/video/audio 走媒体，pdf 走内嵌。
 *  工具栏、保存策略、预览/源码模式由各宿主自行管理，通过 props 传入。 */
import { useEffect, useState } from "react";
import type { ResolvedTheme } from "../../hooks/app/useTheme";
import { resolveMediaSrc } from "../../lib/media/resolveMediaSrc";
import { ChatMarkdown } from "../chat/ChatMarkdown";
import WorkspaceEditor from "../workspace/WorkspaceEditor";
import BrokenMedia from "../media/BrokenMedia";
import MediaPreview from "../media/MediaPreview";
import MediaToolbar from "../media/MediaToolbar";

export type PreviewContentKind =
  | "text"
  | "markdown"
  | "html"
  | "image"
  | "video"
  | "audio"
  | "pdf";

type Props = {
  kind: PreviewContentKind;
  path: string;
  name: string;
  theme: ResolvedTheme;
  /** 可编辑文本（text/markdown/html 源码） */
  draft: string;
  onDraftChange: (value: string) => void;
  /** markdown/html：true=预览，false=源码 */
  previewMode: boolean;
  onOpenExternally: () => void;
  showMediaToolbar?: boolean;
};

export default function FilePreviewContent({
  kind,
  path,
  name,
  theme,
  draft,
  onDraftChange,
  previewMode,
  onOpenExternally,
  showMediaToolbar = true,
}: Props) {
  const [pdfBroken, setPdfBroken] = useState(false);

  useEffect(() => {
    setPdfBroken(false);
  }, [path]);

  if (kind === "image" || kind === "video" || kind === "audio") {
    return (
      <div className="ws-media-stage" data-kind={kind}>
        <div className="ws-media-frame">
          <MediaPreview
            kind={kind}
            path={path}
            alt={name}
            showToolbar={false}
            onOpenExternally={onOpenExternally}
          />
        </div>
      </div>
    );
  }

  if (kind === "pdf") {
    const src = resolveMediaSrc(path);
    if (!src || pdfBroken) {
      return <BrokenMedia path={path} onOpenExternally={onOpenExternally} />;
    }
    return (
      <div className="fs-preview-pdf-wrap" data-kind="document">
        <iframe
          className="fs-preview-pdf"
          title={name}
          src={src}
          onError={() => setPdfBroken(true)}
        />
        {showMediaToolbar ? (
          <MediaToolbar path={path} kind="document" compact />
        ) : null}
      </div>
    );
  }

  const editor = (
    <WorkspaceEditor
      value={draft}
      filename={name}
      theme={theme}
      onChange={onDraftChange}
    />
  );

  if (kind === "markdown") {
    return (
      <div
        className="ws-editor-wrap"
        data-mode={previewMode ? "preview" : "source"}
      >
        {previewMode ? (
          <div className="ws-md-preview">
            <ChatMarkdown content={draft} />
          </div>
        ) : (
          editor
        )}
      </div>
    );
  }

  if (kind === "html") {
    return (
      <div
        className="ws-editor-wrap"
        data-mode={previewMode ? "preview" : "source"}
      >
        {previewMode ? (
          <MediaPreview
            kind="html"
            path={path}
            htmlSource={draft}
            className="ws-editor-html"
          />
        ) : (
          editor
        )}
      </div>
    );
  }

  // text
  return <div className="ws-editor-wrap">{editor}</div>;
}
