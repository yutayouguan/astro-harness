import { useEffect, useState } from "react";
import { Eye, FileCode2, LoaderCircle, Save, X } from "lucide-react";
import type { ResolvedTheme } from "../../hooks/app/useTheme";
import type { ProjectFileWorkbench } from "../../hooks/chat/useProjectFileWorkbench";
import {
  isMarkdownFilename,
  readWorkspaceMdMode,
  writeWorkspaceMdMode,
  type MdMode,
} from "../../lib/filespace/workspaceMdMode";
import { ChatMarkdown } from "./ChatMarkdown";
import FileTypeIcon from "../filespace/FileTypeIcon";
import WorkspaceEditor from "../workspace/WorkspaceEditor";

export default function ProjectFileEditor({
  workbench,
  theme,
}: {
  workbench: ProjectFileWorkbench;
  theme: ResolvedTheme;
}) {
  const { activeTab } = workbench;
  const [mdMode, setMdMode] = useState<MdMode>(readWorkspaceMdMode);
  const isMarkdown = Boolean(activeTab && isMarkdownFilename(activeTab.name));
  const showMarkdownPreview = isMarkdown && mdMode === "preview";

  const toggleMarkdownMode = () => {
    const next = mdMode === "preview" ? "source" : "preview";
    setMdMode(next);
    writeWorkspaceMdMode(next);
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLocaleLowerCase() !== "s") return;
      if (!activeTab) return;
      event.preventDefault();
      void workbench.saveActive();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [activeTab, workbench]);

  if (workbench.tabs.length === 0) return null;

  return (
    <section className="project-file-workbench" aria-label="文件编辑器">
      <div className="project-file-tabs" role="tablist" aria-label="打开的文件">
        <div className="project-file-tabs-scroll">
          {workbench.tabs.map((tab) => {
            const dirty = !tab.readonly && tab.content !== tab.savedContent;
            return (
              <div
                key={tab.key}
                role="tab"
                tabIndex={0}
                aria-selected={workbench.activeKey === tab.key}
                className={`project-file-tab${workbench.activeKey === tab.key ? " is-active" : ""}`}
                onClick={() => workbench.setActiveKey(tab.key)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" || event.key === " ") {
                    event.preventDefault();
                    workbench.setActiveKey(tab.key);
                  }
                }}
                title={tab.path ?? tab.name}
              >
                <FileTypeIcon className="project-file-icon" name={tab.name} size={15} />
                <span>{tab.name}</span>
                {dirty ? <i className="project-file-dirty" aria-label="未保存" /> : null}
                <button
                  type="button"
                  className="project-file-tab-close"
                  aria-label={`关闭 ${tab.name}`}
                  onClick={(event) => {
                    event.stopPropagation();
                    workbench.closeTab(tab.key);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      event.stopPropagation();
                      workbench.closeTab(tab.key);
                    }
                  }}
                >
                  <X size={12} aria-hidden />
                </button>
              </div>
            );
          })}
        </div>
        <div className="project-file-tab-actions">
          {isMarkdown ? (
            <button
              type="button"
              className={showMarkdownPreview ? "is-active" : undefined}
              onClick={toggleMarkdownMode}
              title={showMarkdownPreview ? "显示源码" : "预览 Markdown"}
              aria-label={showMarkdownPreview ? "显示 Markdown 源码" : "预览 Markdown"}
              aria-pressed={showMarkdownPreview}
            >
              {showMarkdownPreview ? (
                <FileCode2 size={14} aria-hidden />
              ) : (
                <Eye size={14} aria-hidden />
              )}
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => void workbench.saveActive()}
            disabled={
              !activeTab ||
              activeTab.readonly ||
              activeTab.loading ||
              activeTab.saving ||
              activeTab.content === activeTab.savedContent
            }
            title={activeTab?.readonly ? "生成中，完成后可编辑" : "保存 (⌘S)"}
            aria-label="保存当前文件"
          >
            {activeTab?.saving ? (
              <LoaderCircle className="project-file-spin" size={14} aria-hidden />
            ) : (
              <Save size={14} aria-hidden />
            )}
          </button>
          <button
            type="button"
            onClick={workbench.closeAll}
            title="关闭全部文件"
            aria-label="关闭全部文件"
          >
            <X size={14} aria-hidden />
          </button>
        </div>
      </div>
      <div className="project-file-editor-body">
        {activeTab?.loading ? (
          <div className="project-file-editor-state" role="status">
            <LoaderCircle className="project-file-spin" size={22} aria-hidden />
            <span>正在打开文件…</span>
          </div>
        ) : activeTab?.error ? (
          <div className="project-file-editor-state is-error" role="alert">
            <FileCode2 size={22} aria-hidden />
            <strong>无法打开文件</strong>
            <span>{activeTab.error}</span>
          </div>
        ) : activeTab ? (
          <>
            {activeTab.transient ? (
              <div className="project-file-streaming-badge">
                {activeTab.readonly ? "Agent 正在生成" : "生成快照"}
              </div>
            ) : null}
            {showMarkdownPreview ? (
              <div className="project-file-markdown-preview">
                <ChatMarkdown
                  content={activeTab.content}
                  mediaBaseDir={activeTab.path?.replace(/[\\/][^\\/]*$/, "")}
                />
              </div>
            ) : (
              <WorkspaceEditor
                value={activeTab.content}
                filename={activeTab.name}
                theme={theme}
                onChange={workbench.updateActiveContent}
              />
            )}
          </>
        ) : null}
      </div>
    </section>
  );
}
