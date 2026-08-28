import { useEffect, useMemo, useState } from "react";
import { ChevronDown as ChevronDownData, ChevronUp as ChevronUpData } from "lucide";
import { Layers3 } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  distinctActivityVisualKinds,
  type ActivityVisualKind,
} from "../../lib/chat/activityPresentation";
import type { ChatActivity } from "../../types";
import { MorphToggleIcon } from "../icons/MorphIcon";
import MsgActivity from "./MsgActivity";

type Props = {
  activities: ChatActivity[];
  defaultOpen?: boolean;
  showTimestamp: boolean;
  mediaBaseDir?: string | null;
};

export default function MsgActivityGroup({
  activities,
  defaultOpen = false,
  showTimestamp,
  mediaBaseDir,
}: Props) {
  const { t } = useI18n();
  const running = activities.some((activity) => activity.status === "running");
  const failed = activities.some((activity) => activity.status === "error");
  const interrupted = activities.some(
    (activity) => activity.status === "interrupted",
  );
  const [open, setOpen] = useState(
    () => defaultOpen || running || failed || interrupted,
  );

  useEffect(() => {
    if (running || failed || interrupted) setOpen(true);
  }, [running, failed, interrupted]);

  const actionLabel = useMemo(
    () =>
      distinctActivityVisualKinds(activities)
        .map((kind) => activityKindLabel(kind, t))
        .join(" · "),
    [activities, t],
  );
  const stateLabel = running
    ? t("chat.activity.status.running")
    : failed
      ? t("chat.activity.status.error")
      : interrupted
        ? t("chat.activity.status.interrupted")
        : null;

  return (
    <section
      className={`msg-activity-group${open ? " is-open" : ""}${
        running ? " is-running" : ""
      }${failed ? " is-error" : ""}${
        interrupted ? " is-interrupted" : ""
      }`}
    >
      <button
        type="button"
        className="msg-activity-group-toggle"
        aria-expanded={open}
        aria-label={open ? t("chat.activityGroup.collapse") : t("chat.activityGroup.expand")}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="msg-activity-group-icon" aria-hidden>
          <Layers3 size={14} strokeWidth={2} />
        </span>
        <span className="msg-activity-group-title">{actionLabel}</span>
        <span className="msg-activity-group-count">
          {t("chat.activityGroup.count", { count: String(activities.length) })}
          {stateLabel ? ` · ${stateLabel}` : ""}
        </span>
        <MorphToggleIcon
          active={open}
          activeIcon={ChevronUpData}
          inactiveIcon={ChevronDownData}
          size={14}
          strokeWidth={2}
          className="msg-activity-group-chevron"
          aria-hidden
        />
      </button>
      <div className="msg-activity-group-collapse">
        <div className="msg-activity-group-items">
          {activities.map((activity) => (
            <MsgActivity
              key={activity.id}
              activity={activity}
              defaultOpen={
                activity.status === "error" || activity.status === "interrupted"
              }
              showTimestamp={showTimestamp}
              mediaBaseDir={mediaBaseDir}
            />
          ))}
        </div>
      </div>
    </section>
  );
}

function activityKindLabel(
  kind: ActivityVisualKind,
  t: ReturnType<typeof useI18n>["t"],
): string {
  switch (kind) {
    case "read":
      return t("chat.activity.action.read");
    case "search":
      return t("chat.activity.action.search");
    case "run":
      return t("chat.activity.action.run");
    case "edit":
      return t("chat.activity.action.edit");
    case "browse":
      return t("chat.activity.action.browse");
    case "media":
      return t("chat.activity.action.media");
    case "skill":
      return t("chat.activity.kind.skill");
    case "mcp":
      return t("chat.activity.kind.mcp");
    case "hook":
      return t("chat.activity.kind.hook");
    case "memory":
      return t("chat.activity.kind.memory");
    case "status":
      return t("chat.activity.kind.status");
    case "tool":
      return t("chat.activity.kind.tool");
  }
}
