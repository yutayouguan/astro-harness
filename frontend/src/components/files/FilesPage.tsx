/** 「文件」页外壳：浏览（工作区目录）/ 产物（对话产物索引）分段切换，右侧复用各自预览。 */
import { useState } from "react";
import { FolderTree, Sparkles } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import type { ArtifactDto } from "../../types";
import WorkspacePanel from "../workspace/WorkspacePanel";
import FileSpacePanel from "../filespace/FileSpacePanel";

type AttachMode = "new" | "current";

type FilesSubmode = "browse" | "artifacts";

const SUBMODE_KEY = "astro.files.submode";

function readSubmode(): FilesSubmode {
  try {
    const v = localStorage.getItem(SUBMODE_KEY);
    if (v === "browse" || v === "artifacts") return v;
  } catch {
    // ignore
  }
  return "browse";
}

function writeSubmode(mode: FilesSubmode): void {
  try {
    localStorage.setItem(SUBMODE_KEY, mode);
  } catch {
    // ignore
  }
}

type Props = {
  active: boolean;
  onOpenSession: (sessionId: string, messageId?: string | null) => void;
  onAttachFiles?: (files: ArtifactDto[], mode: AttachMode) => void | Promise<void>;
  onClose?: () => void;
};

export default function FilesPage({
  active,
  onOpenSession,
  onAttachFiles,
  onClose,
}: Props) {
  const { t } = useI18n();
  const [submode, setSubmode] = useState<FilesSubmode>(() => readSubmode());
  const [pendingOpenPath, setPendingOpenPath] = useState<string | null>(null);

  const select = (mode: FilesSubmode) => {
    setSubmode(mode);
    writeSubmode(mode);
  };

  const openInWorkspace = (path: string) => {
    setPendingOpenPath(path);
    setSubmode("browse");
    writeSubmode("browse");
  };

  return (
    <div className="files-page">
      <div className="files-mode-switch" role="tablist" aria-label={t("files.mode")}>
        <button
          type="button"
          role="tab"
          aria-selected={submode === "browse"}
          className={`files-mode-tab ${submode === "browse" ? "is-active" : ""}`}
          onClick={() => select("browse")}
        >
          <FolderTree size={15} strokeWidth={2.2} aria-hidden />
          {t("files.mode.browse")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={submode === "artifacts"}
          className={`files-mode-tab ${submode === "artifacts" ? "is-active" : ""}`}
          onClick={() => select("artifacts")}
        >
          <Sparkles size={15} strokeWidth={2.2} aria-hidden />
          {t("files.mode.artifacts")}
        </button>
      </div>

      <div className="files-mode-body">
        {submode === "browse" ? (
          <WorkspacePanel
            onClose={onClose}
            openPath={pendingOpenPath}
            onDidOpenPath={() => setPendingOpenPath(null)}
          />
        ) : (
          <FileSpacePanel
            active={active && submode === "artifacts"}
            onOpenSession={onOpenSession}
            onAttachFiles={onAttachFiles}
            onOpenInWorkspace={openInWorkspace}
            onClose={onClose}
          />
        )}
      </div>
    </div>
  );
}
