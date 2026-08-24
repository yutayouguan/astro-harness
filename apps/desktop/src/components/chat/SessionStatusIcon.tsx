import { CircleAlert, CircleCheck, LoaderCircle, ShieldAlert } from "lucide-react";

/** 会话在列表中的活动状态 */
export type SessionActivityStatus = "idle" | "running" | "awaiting" | "error";

/** 同一会话可能同时命中多种信号，按紧急度取一个 */
export function resolveSessionStatus(
  sessionId: string,
  ids: {
    streamingSessionId?: string | null;
    awaitingSessionId?: string | null;
    errorSessionId?: string | null;
  },
): SessionActivityStatus {
  if (sessionId === ids.awaitingSessionId) return "awaiting";
  if (sessionId === ids.errorSessionId) return "error";
  if (sessionId === ids.streamingSessionId) return "running";
  return "idle";
}

type Props = {
  status: SessionActivityStatus;
  /** 无障碍标签 / 悬浮提示；不传则图标对读屏隐藏 */
  label?: string;
};

export default function SessionStatusIcon({ status, label }: Props) {
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
        <CircleCheck
          className="session-status-icon-complete"
          size={14}
          strokeWidth={2}
        />
      )}
    </span>
  );
}
