/** 「工作空间」页外壳：浏览（工作区目录）/ 产物（对话产物索引）由页头分段切换控制。
 *  两侧面板常驻挂载、仅显隐切换，切回时保留各自滚动/选中/已打开文件等状态。 */
import { useState } from "react";
import type { ArtifactDto } from "../../types";
import type { FilesSubmode } from "../../lib/filespace/filesMode";
import WorkspacePanel from "../workspace/WorkspacePanel";
import FileSpacePanel from "../filespace/FileSpacePanel";

type AttachMode = "new" | "current";

type Props = {
  active: boolean;
  /** 当前子模式（受控，由页头分段切换） */
  submode: FilesSubmode;
  onSubmodeChange: (mode: FilesSubmode) => void;
  onOpenSession: (sessionId: string, messageId?: string | null) => void;
  onAttachFiles?: (
    files: ArtifactDto[],
    mode: AttachMode,
  ) => void | Promise<void>;
  onClose?: () => void;
};

export default function FilesPage({
  active,
  submode,
  onSubmodeChange,
  onOpenSession,
  onAttachFiles,
  onClose,
}: Props) {
  const [pendingOpenPath, setPendingOpenPath] = useState<string | null>(null);

  const openInWorkspace = (path: string) => {
    setPendingOpenPath(path);
    onSubmodeChange("browse");
  };

  return (
    <div className="files-page">
      <div
        className={`files-mode-pane ${submode === "browse" ? "" : "is-hidden"}`}
      >
        <WorkspacePanel
          onClose={onClose}
          openPath={pendingOpenPath}
          onDidOpenPath={() => setPendingOpenPath(null)}
          onOpenSession={onOpenSession}
        />
      </div>
      <div
        className={`files-mode-pane ${submode === "artifacts" ? "" : "is-hidden"}`}
      >
        <FileSpacePanel
          active={active && submode === "artifacts"}
          onOpenSession={onOpenSession}
          onAttachFiles={onAttachFiles}
          onOpenInWorkspace={openInWorkspace}
          onClose={onClose}
        />
      </div>
    </div>
  );
}
