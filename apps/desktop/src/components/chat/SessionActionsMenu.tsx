/** 当前会话操作菜单：标题栏与侧栏共用同一套能力、状态与执行路径。 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import {
  ChevronRight,
  Download,
  Edit3,
  FolderInput,
  GitBranch,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  Archive as ArchiveData,
  ArchiveRestore as ArchiveRestoreData,
  Pin as PinData,
  PinOff as PinOffData,
} from "lucide";

import { useAppDialog } from "../../hooks/ui/DialogContext";
import type { ShowToastOptions } from "../../hooks/ui/useTransientToast";
import { useI18n } from "../../i18n/LocaleContext";
import {
  deleteManagedSession,
  dispatchSessionsChanged,
} from "../../lib/chat/sessionManagement";
import {
  clampPopover,
  measurePopoverSize,
  pointAnchor,
  resolveClipBoundsAt,
} from "../../lib/ui/clampPopover";
import { projectResponseItemsToEntries } from "../../lib/chat/projectResponseItemsToEntries";
import type {
  ResponseItemHistoryDto,
  ConversationEntry,
  ProjectDto,
  RecentSessionDto,
} from "../../types";
import { MorphToggleIcon } from "../icons/MorphIcon";
import type { SessionActivityStatus } from "./SessionStatusIcon";

type Props = {
  session: RecentSessionDto;
  x: number;
  y: number;
  status: SessionActivityStatus;
  activeSessionId: string | null;
  onClose: () => void;
  onOpenSession: (sessionId: string) => void;
  showToast: (message: string, options?: ShowToastOptions) => void;
  onPrepareDeleteCurrentSession?: () => void | Promise<void>;
  onClearDeletedCurrentSession?: () => void | Promise<void>;
};

function sanitizeExportFilename(title: string, sessionId: string): string {
  const base = title
    .replace(/[\\/:*?"<>|]+/g, "_")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 48);
  return `${base || "session"}-${sessionId.slice(0, 8)}.md`;
}

function utf8ToBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  const chunk = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunk) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunk));
  }
  return btoa(binary);
}

function historyToMarkdown(
  title: string,
  sessionId: string,
  messages: Pick<ConversationEntry, "role" | "content">[],
): string {
  const lines = [`# ${title}`, "", `> session: \`${sessionId}\``, ""];
  for (const message of messages) {
    const role = message.role.trim() || "message";
    const content = (message.content ?? "").trim();
    if (!content) continue;
    lines.push(`## ${role}`, "", content, "");
  }
  return `${lines.join("\n").trim()}\n`;
}

export default function SessionActionsMenu({
  session,
  x,
  y,
  status,
  activeSessionId,
  onClose,
  onOpenSession,
  showToast,
  onPrepareDeleteCurrentSession,
  onClearDeletedCurrentSession,
}: Props) {
  const { t } = useI18n();
  const { confirm, prompt } = useAppDialog();
  const menuRef = useRef<HTMLDivElement>(null);
  const [moveProjectOpen, setMoveProjectOpen] = useState(false);
  const [moveProjects, setMoveProjects] = useState<ProjectDto[]>([]);
  const [moveProjectsLoading, setMoveProjectsLoading] = useState(false);

  useLayoutEffect(() => {
    const element = menuRef.current;
    if (!element) return;

    const positionMenu = () => {
      const size = measurePopoverSize(element);
      const bounds = resolveClipBoundsAt(x, y);
      const spaceBelow = bounds.bottom - 8 - y;
      const spaceAbove = y - bounds.top - 8;
      const placement =
        size.height > spaceBelow && spaceAbove > spaceBelow ? "above" : "below";
      const position = clampPopover({
        anchorRect: pointAnchor(x, y),
        popoverSize: size,
        bounds,
        preferAlign: "start",
        placement,
        gap: 0,
        pad: 8,
      });
      element.style.left = `${position.left}px`;
      element.style.top = `${position.top}px`;
      element.style.maxHeight = `${position.maxHeight}px`;
      element.style.overflowY = position.maxHeight < size.height ? "auto" : "";
    };

    positionMenu();
    window.addEventListener("resize", positionMenu);
    return () => window.removeEventListener("resize", positionMenu);
  }, [moveProjectOpen, moveProjects.length, x, y]);

  useEffect(() => {
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!menuRef.current?.contains(event.target as Node)) onClose();
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", closeOnPointerDown);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointerDown);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [onClose]);

  const runSessionAction = useCallback(
    async (action: () => Promise<void>) => {
      onClose();
      try {
        await action();
        dispatchSessionsChanged();
      } catch (error) {
        showToast(
          t("sessions.actionFailed", {
            error: error instanceof Error ? error.message : String(error),
          }),
          { error: true },
        );
      }
    },
    [onClose, showToast, t],
  );

  const handleRename = useCallback(async () => {
    onClose();
    const current = session.summary || t("chat.rightPanel.untitledSession");
    const next = await prompt({
      title: t("sessions.rename"),
      message: t("sessions.renamePrompt"),
      defaultValue: current,
      confirmLabel: t("sessions.renameSave"),
      cancelLabel: t("sessions.cancel"),
    });
    const title = next?.trim();
    if (!title || title === current) return;
    await runSessionAction(async () => {
      await invoke("rename_session", { sessionId: session.sessionId, title });
    });
  }, [onClose, prompt, runSessionAction, session, t]);

  const handleExport = useCallback(() => {
    void runSessionAction(async () => {
      const history = await invoke<ResponseItemHistoryDto>("get_chat_history", {
        sessionId: session.sessionId,
        limit: 500,
      });
      const title = session.summary || t("chat.rightPanel.untitledSession");
      const markdown = historyToMarkdown(
        title,
        session.sessionId,
        projectResponseItemsToEntries(history.items ?? []),
      );
      if (!markdown.replace(/^#.*$/m, "").trim()) {
        throw new Error(t("sessions.exportEmpty"));
      }
      const savedPath = await invoke<string>("download_bytes_to_downloads", {
        filename: sanitizeExportFilename(title, session.sessionId),
        base64Data: utf8ToBase64(markdown),
      });
      showToast(t("sessions.exportDone", { path: savedPath }), {
        tone: "success",
      });
    });
  }, [runSessionAction, session, showToast, t]);

  const handleBranch = useCallback(() => {
    void runSessionAction(async () => {
      const history = await invoke<ResponseItemHistoryDto>("get_chat_history", {
        sessionId: session.sessionId,
        limit: 500,
      });
      const bubbles = projectResponseItemsToEntries(history.items ?? []);
      if (bubbles.length === 0) throw new Error(t("sessions.branchEmpty"));
      const newId = await invoke<string>("fork_chat_session", {
        sourceSessionId: session.sessionId,
        keepChatBubbles: bubbles.length,
      });
      onOpenSession(newId);
    });
  }, [onOpenSession, runSessionAction, session.sessionId, t]);

  const toggleMoveProjects = useCallback(async () => {
    const nextOpen = !moveProjectOpen;
    setMoveProjectOpen(nextOpen);
    if (!nextOpen || moveProjectsLoading) return;
    setMoveProjectsLoading(true);
    try {
      setMoveProjects(await invoke<ProjectDto[]>("list_projects"));
    } catch (error) {
      showToast(
        t("sessions.actionFailed", {
          error: error instanceof Error ? error.message : String(error),
        }),
        { error: true },
      );
      setMoveProjectOpen(false);
    } finally {
      setMoveProjectsLoading(false);
    }
  }, [moveProjectOpen, moveProjectsLoading, showToast, t]);

  const handleDelete = useCallback(async () => {
    onClose();
    const title = session.summary || t("chat.rightPanel.untitledSession");
    const confirmed = await confirm({
      title: t("sessions.deleteTitle"),
      emphasisLabel: t("sessions.deleteTargetLabel"),
      emphasis: title,
      message: t("sessions.deleteConfirm"),
      confirmLabel: t("sessions.deletePermanently"),
      cancelLabel: t("sessions.cancel"),
      variant: "danger",
    });
    if (!confirmed) return;
    await runSessionAction(async () => {
      if (session.sessionId === activeSessionId) {
        await onPrepareDeleteCurrentSession?.();
      }
      await deleteManagedSession(
        session.sessionId,
        activeSessionId,
        async () => {
          await invoke("delete_session_permanently", {
            sessionId: session.sessionId,
          });
        },
        async () => {
          await onClearDeletedCurrentSession?.();
        },
      );
    });
  }, [
    activeSessionId,
    confirm,
    onClearDeletedCurrentSession,
    onClose,
    onPrepareDeleteCurrentSession,
    runSessionAction,
    session,
    t,
  ]);

  const archived = Boolean(session.archivedAt);
  const pinned = Boolean(session.pinnedAt);
  const moveDisabled = status === "running" || status === "awaiting";
  const targetProjects = moveProjects.filter(
    (candidate) => candidate.id !== session.projectId,
  );

  return createPortal(
    <div
      ref={menuRef}
      className="project-context-menu"
      style={{ top: y, left: x }}
      role="menu"
    >
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={() =>
          void runSessionAction(async () => {
            await invoke(pinned ? "unpin_session" : "pin_session", {
              sessionId: session.sessionId,
            });
          })
        }
      >
        <MorphToggleIcon
          active={pinned}
          activeIcon={PinOffData}
          inactiveIcon={PinData}
          size={14}
          strokeWidth={1.8}
          aria-hidden
        />
        <span>{pinned ? t("sessions.unpin") : t("sessions.pin")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={() => void handleRename()}
      >
        <Edit3 size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.rename")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={() =>
          void runSessionAction(async () => {
            await invoke("regenerate_session_title", {
              sessionId: session.sessionId,
            });
          })
        }
      >
        <RefreshCw size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.regenerateTitle")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={handleExport}
      >
        <Download size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.export")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={handleBranch}
      >
        <GitBranch size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.branch")}</span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        disabled={moveDisabled}
        title={moveDisabled ? t("sessions.moveRunningDisabled") : undefined}
        aria-expanded={moveProjectOpen}
        onClick={() => void toggleMoveProjects()}
      >
        <FolderInput size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.moveToProject")}</span>
        <ChevronRight
          className={`session-move-project-chevron ${moveProjectOpen ? "is-open" : ""}`}
          size={13}
          strokeWidth={2}
          aria-hidden
        />
      </button>
      {moveProjectOpen ? (
        <div
          className="session-move-project-list"
          role="group"
          aria-label={t("sessions.moveToProject")}
        >
          {moveProjectsLoading ? (
            <span className="session-move-project-empty">
              {t("sessions.moveLoading")}
            </span>
          ) : targetProjects.length === 0 ? (
            <span className="session-move-project-empty">
              {t("sessions.noOtherProjects")}
            </span>
          ) : (
            targetProjects.map((target) => (
              <button
                key={target.id}
                type="button"
                role="menuitem"
                className="project-context-menu-item session-move-project-target"
                onClick={() =>
                  void runSessionAction(async () => {
                    await invoke("assign_session_to_project", {
                      sessionId: session.sessionId,
                      projectId: target.id,
                    });
                    if (session.sessionId === activeSessionId) {
                      await onClearDeletedCurrentSession?.();
                    }
                    showToast(
                      t("sessions.moveDone", { project: target.name }),
                      {
                        tone: "success",
                      },
                    );
                  })
                }
              >
                <FolderInput size={13} strokeWidth={1.8} aria-hidden />
                <span>{target.name}</span>
              </button>
            ))
          )}
        </div>
      ) : null}
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item"
        onClick={() =>
          void runSessionAction(async () => {
            await invoke(archived ? "unarchive_session" : "archive_session", {
              sessionId: session.sessionId,
            });
          })
        }
      >
        <MorphToggleIcon
          active={archived}
          activeIcon={ArchiveRestoreData}
          inactiveIcon={ArchiveData}
          size={14}
          strokeWidth={1.8}
          aria-hidden
        />
        <span>
          {archived ? t("sessions.unarchive") : t("sessions.archive")}
        </span>
      </button>
      <button
        type="button"
        role="menuitem"
        className="project-context-menu-item is-danger"
        onClick={() => void handleDelete()}
      >
        <Trash2 size={14} strokeWidth={1.8} aria-hidden />
        <span>{t("sessions.deletePermanently")}</span>
      </button>
    </div>,
    document.body,
  );
}
