/** 文件空间右侧：文本编辑 / MD·HTML 预览 / 图音视 / PDF */
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { Eye, FileCode2, Save } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import { useTheme } from "../hooks/useTheme";
import { filespaceViewerKind } from "../lib/filespaceViewerKind";
import { resolveMediaSrc } from "../lib/resolveMediaSrc";
import { ChatMarkdown } from "./ChatMarkdown";
import WorkspaceEditor from "./WorkspaceEditor";
import BrokenMedia from "./media/BrokenMedia";
import MediaPreview from "./media/MediaPreview";
import MediaToolbar from "./media/MediaToolbar";

const AUTOSAVE_MS = 600;

type Props = {
  path: string;
  name: string;
  missing?: boolean;
  mime?: string | null;
  category?: string | null;
  onOpenExternally: () => void;
};

type DocMode = "preview" | "source";

type TextState = {
  path: string;
  draft: string;
  lastSaved: string;
  loading: boolean;
  saving: boolean;
  error: string | null;
};

export default function FileSpaceViewer({
  path,
  name,
  missing,
  mime,
  category,
  onOpenExternally,
}: Props) {
  const { t } = useI18n();
  const { resolved: theme } = useTheme();
  const kind = filespaceViewerKind({ name, missing, mime, category });

  const [docMode, setDocMode] = useState<DocMode>("preview");
  const [text, setText] = useState<TextState | null>(null);
  const [pdfBroken, setPdfBroken] = useState(false);

  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const textRef = useRef<TextState | null>(null);
  textRef.current = text;

  const isEditable = kind === "text" || kind === "markdown" || kind === "html";

  const clearTimer = () => {
    if (saveTimer.current) {
      clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }
  };

  const writeNow = useCallback(
    async (targetPath: string, content: string): Promise<boolean> => {
      try {
        await invoke("write_file", { path: targetPath, content });
        setText((prev) =>
          prev && prev.path === targetPath
            ? { ...prev, lastSaved: content, saving: false, error: null }
            : prev,
        );
        return true;
      } catch (e) {
        setText((prev) =>
          prev && prev.path === targetPath
            ? { ...prev, saving: false, error: String(e) }
            : prev,
        );
        return false;
      }
    },
    [],
  );

  const flushSave = useCallback(async (): Promise<boolean> => {
    clearTimer();
    const cur = textRef.current;
    if (!cur || cur.draft === cur.lastSaved) return true;
    setText((prev) => (prev ? { ...prev, saving: true } : prev));
    return writeNow(cur.path, cur.draft);
  }, [writeNow]);

  const scheduleSave = useCallback(
    (targetPath: string, content: string) => {
      clearTimer();
      saveTimer.current = setTimeout(() => {
        saveTimer.current = null;
        setText((prev) => {
          if (!prev || prev.path !== targetPath || prev.draft !== content) {
            return prev;
          }
          if (content === prev.lastSaved) return prev;
          return { ...prev, saving: true };
        });
        void writeNow(targetPath, content);
      }, AUTOSAVE_MS);
    },
    [writeNow],
  );

  /** 切换文件：flush 后加载 */
  useEffect(() => {
    let cancelled = false;

    const run = async () => {
      const prev = textRef.current;
      if (prev && prev.draft !== prev.lastSaved) {
        const ok = await flushSave();
        if (!ok) {
          // 选中已由 Panel 切换；写盘失败时提示，确认后丢弃未保存内容
          window.confirm(t("workspace.unsavedConfirm"));
        }
      }
      clearTimer();
      setDocMode(kind === "text" ? "source" : "preview");
      setPdfBroken(false);

      if (!isEditable || missing) {
        setText(null);
        return;
      }

      setText({
        path,
        draft: "",
        lastSaved: "",
        loading: true,
        saving: false,
        error: null,
      });

      try {
        const content = await invoke<string>("read_file", { path });
        if (cancelled) return;
        setText({
          path,
          draft: content,
          lastSaved: content,
          loading: false,
          saving: false,
          error: null,
        });
      } catch (e) {
        if (cancelled) return;
        setText({
          path,
          draft: "",
          lastSaved: "",
          loading: false,
          saving: false,
          error: String(e),
        });
      }
    };

    void run();
    return () => {
      cancelled = true;
    };
    // 仅 path/kind/missing 驱动重载；flushSave/t 稳定用途
    // eslint-disable-next-line react-hooks/exhaustive-deps -- intentional path switch
  }, [path, kind, missing]);

  /** 卸载时尽量写盘 */
  useEffect(() => {
    return () => {
      clearTimer();
      const cur = textRef.current;
      if (cur && cur.draft !== cur.lastSaved) {
        void invoke("write_file", {
          path: cur.path,
          content: cur.draft,
        }).catch(() => {});
      }
    };
  }, []);

  const onDraftChange = (value: string) => {
    setText((prev) => {
      if (!prev) return prev;
      const next = { ...prev, draft: value, error: null };
      scheduleSave(prev.path, value);
      return next;
    });
  };

  const saveManual = () => {
    void flushSave();
  };

  const onKeyDown = (e: KeyboardEvent) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "s") {
      e.preventDefault();
      void flushSave();
    }
  };

  if (kind === "missing") {
    return <div className="fs-empty">{t("filespace.missing")}</div>;
  }

  if (kind === "external") {
    return (
      <div className="fs-empty">
        <p>{t("filespace.unsupportedHint")}</p>
        <p className="fs-preview-path">{path}</p>
        <button
          type="button"
          className="fs-action-btn is-primary"
          onClick={onOpenExternally}
        >
          {t("workspace.openExternally")}
        </button>
      </div>
    );
  }

  if (kind === "image" || kind === "video" || kind === "audio") {
    return (
      <MediaPreview
        kind={kind}
        path={path}
        alt={name}
        className="fs-viewer-media"
      />
    );
  }

  if (kind === "pdf") {
    const src = resolveMediaSrc(path);
    if (!src || pdfBroken) {
      return (
        <BrokenMedia path={path} onOpenExternally={onOpenExternally} />
      );
    }
    return (
      <div className="fs-preview-pdf-wrap" data-kind="document">
        <iframe
          className="fs-preview-pdf"
          title={name}
          src={src}
          onError={() => setPdfBroken(true)}
        />
        <MediaToolbar path={path} kind="document" />
      </div>
    );
  }

  // text / markdown / html
  if (!text || text.loading) {
    return <div className="fs-status">…</div>;
  }

  if (text.error && !text.draft && text.lastSaved === "") {
    return (
      <div className="fs-empty">
        <p>{text.error}</p>
        <button
          type="button"
          className="fs-action-btn is-primary"
          onClick={onOpenExternally}
        >
          {t("workspace.openExternally")}
        </button>
      </div>
    );
  }

  const dirty = text.draft !== text.lastSaved;
  const showModeToggle = kind === "markdown" || kind === "html";
  const showPreviewOnly = showModeToggle && docMode === "preview";
  const showSource = !showModeToggle || docMode === "source";

  return (
    <div className="fs-viewer" onKeyDown={onKeyDown}>
      <div className="fs-viewer-toolbar">
        {showModeToggle && (
          <div className="ws-md-modes" role="tablist">
            <button
              type="button"
              role="tab"
              aria-selected={docMode === "preview"}
              className={`ws-md-mode ${docMode === "preview" ? "is-active" : ""}`}
              onClick={() => setDocMode("preview")}
            >
              <Eye size={13} strokeWidth={2.3} aria-hidden />
              {t("workspace.previewMode")}
            </button>
            <button
              type="button"
              role="tab"
              aria-selected={docMode === "source"}
              className={`ws-md-mode ${docMode === "source" ? "is-active" : ""}`}
              onClick={() => setDocMode("source")}
            >
              <FileCode2 size={13} strokeWidth={2.3} aria-hidden />
              {t("workspace.previewSource")}
            </button>
          </div>
        )}
        <div className="fs-viewer-toolbar-spacer" />
        {dirty && (
          <span className="ws-dirty-badge">{t("workspace.unsaved")}</span>
        )}
        {text.saving && (
          <span className="fs-viewer-saving">{t("filespace.saving")}</span>
        )}
        <button
          type="button"
          className="fs-action-btn is-primary"
          disabled={!dirty || text.saving}
          onClick={saveManual}
        >
          <Save size={14} strokeWidth={2.1} aria-hidden />
          {t("workspace.save")}
        </button>
      </div>

      {text.error && <div className="fs-viewer-error">{text.error}</div>}

      {showPreviewOnly && kind === "markdown" && (
        <div className="fs-viewer-preview ws-md-preview">
          <ChatMarkdown content={text.draft} />
        </div>
      )}

      {showPreviewOnly && kind === "html" && (
        <MediaPreview
          kind="html"
          path={path}
          htmlSource={text.draft}
          className="fs-viewer-html"
        />
      )}

      {showSource && (
        <div
          className={
            showModeToggle
              ? "fs-viewer-split"
              : "fs-viewer-editor-wrap"
          }
        >
          <div className="fs-viewer-editor">
            <WorkspaceEditor
              value={text.draft}
              filename={name}
              theme={theme}
              onChange={onDraftChange}
            />
          </div>
          {showModeToggle && kind === "markdown" && (
            <div className="fs-viewer-live-preview ws-md-preview">
              <ChatMarkdown content={text.draft} />
            </div>
          )}
          {showModeToggle && kind === "html" && (
            <div className="fs-viewer-live-preview">
              <MediaPreview
                kind="html"
                path={path}
                htmlSource={text.draft}
                className="fs-viewer-html"
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
}
