import { CircleAlert, LoaderCircle, ShieldAlert } from "lucide-react";
import type { SessionRuntimeStatus } from "../../hooks/chat/useSessionStatusMap";

/** 会话在列表中的活动状态 */
export type SessionActivityStatus = "idle" | "running" | "awaiting" | "error";

export function resolveSessionStatus(
  runtime: SessionRuntimeStatus | undefined,
): SessionActivityStatus {
  if (runtime?.status === "systemError") return "error";
  if (
    runtime?.status === "active" &&
    runtime.activeFlags.some(
      (flag) =>
        flag === "waitingOnApproval" || flag === "waitingOnUserInput",
    )
  ) {
    return "awaiting";
  }
  if (runtime?.status === "active") return "running";
  return "idle";
}

type Props = {
  status: SessionActivityStatus;
  unread?: boolean;
  /** 无障碍标签 / 悬浮提示；不传则图标对读屏隐藏 */
  label?: string;
};

export default function SessionStatusIcon({ status, unread = false, label }: Props) {
  if (status === "idle" && !unread) return null;

  const a11y = label
    ? { role: "img" as const, "aria-label": label, title: label }
    : { "aria-hidden": true };

  return (
    <span className="session-status-icon" {...a11y}>
      {status === "running" ? (
        <LoaderCircle
          className="session-status-icon-spin"
          size={14}
          strokeWidth={2.2}
        />
      ) : status === "awaiting" ? (
        <ShieldAlert
          className="session-status-icon-awaiting"
          size={14}
          strokeWidth={2.2}
        />
      ) : status === "error" ? (
        <CircleAlert
          className="session-status-icon-error"
          size={14}
          strokeWidth={2.2}
        />
      ) : (
        <span className="session-status-unread-dot" aria-hidden />
      )}
    </span>
  );
}
