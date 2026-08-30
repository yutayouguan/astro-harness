import { useEffect, useMemo, useState } from "react";
import { ChevronDown as ChevronDownData, ChevronUp as ChevronUpData } from "lucide";
import { Layers3 } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  activityGroupSummary,
  type ActivityGroupSummary,
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
    () => activityGroupSummaryLabel(activityGroupSummary(activities), t),
    [activities, t],
  );
  const completedCount = activities.filter(
    (activity) => activity.status === "done",
  ).length;
  const failedCount = activities.filter(
    (activity) => activity.status === "error",
  ).length;
  const parallel =
    activities.length > 1 &&
    activities.every((activity) => activity.executionMode === "parallel");
  const metadata = [
    running || failed || interrupted
      ? t("chat.activityGroup.progress", {
          completed: String(completedCount),
          total: String(activities.length),
        })
      : t("chat.activityGroup.completed", { count: String(activities.length) }),
    parallel
      ? t("chat.activityGroup.parallel", { count: String(activities.length) })
      : null,
    running
      ? t("chat.activity.status.running")
      : failed
        ? t("chat.activityGroup.failed", { count: String(failedCount) })
        : interrupted
          ? t("chat.activity.status.interrupted")
          : null,
  ].filter(Boolean);

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
          {metadata.join(" · ")}
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

function activityGroupSummaryLabel(
  summary: ActivityGroupSummary,
  t: ReturnType<typeof useI18n>["t"],
): string {
  return t(`chat.activityGroup.summary.${summary}`);
}
