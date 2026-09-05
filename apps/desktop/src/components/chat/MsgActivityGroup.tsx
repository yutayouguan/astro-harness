import { useEffect, useMemo, useState } from "react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { Layers3 } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  activityGroupProgress,
  activityGroupSummary,
  type ActivityGroupSummary,
} from "../../lib/chat/activityPresentation";
import type { ChatActivity } from "../../types";
import { MorphToggleIcon } from "../icons/MorphIcon";
import MsgActivity from "./MsgActivity";

type Props = {
  activities: ChatActivity[];
  defaultOpen?: boolean;
  forcedOpen?: boolean;
  showTimestamp: boolean;
  mediaBaseDir?: string | null;
};

export default function MsgActivityGroup({
  activities,
  defaultOpen = false,
  forcedOpen,
  showTimestamp,
  mediaBaseDir,
}: Props) {
  const { t } = useI18n();
  const progress = activityGroupProgress(activities);
  const waiting = progress.waiting > 0;
  const running = progress.running > 0;
  const retrying = progress.retrying > 0;
  const failed = progress.error > 0;
  const declined = progress.declined > 0;
  const interrupted = progress.interrupted > 0;
  const partial = progress.hasPartialOutcome;
  const needsAttention =
    waiting ||
    running ||
    retrying ||
    failed ||
    declined ||
    interrupted ||
    partial;
  const [open, setOpen] = useState(() => defaultOpen || needsAttention);

  useEffect(() => {
    if (forcedOpen != null) {
      setOpen(forcedOpen);
    } else if (needsAttention) {
      setOpen(true);
    }
  }, [forcedOpen, needsAttention]);

  const actionLabel = useMemo(
    () => activityGroupSummaryLabel(activityGroupSummary(activities), t),
    [activities, t],
  );
  const parallel =
    activities.length > 1 &&
    activities.every((activity) => activity.executionMode === "parallel");
  const metadata = [
    partial
      ? t("chat.activityGroup.partial", {
          completed: String(progress.done),
          total: String(progress.total),
        })
      : waiting || running || retrying || failed || declined || interrupted
        ? t("chat.activityGroup.progress", {
            completed: String(progress.done),
            total: String(progress.total),
          })
        : t("chat.activityGroup.completed", {
            count: String(activities.length),
          }),
    parallel
      ? t("chat.activityGroup.parallel", { count: String(activities.length) })
      : null,
    waiting
      ? t("chat.activityGroup.waiting", { count: String(progress.waiting) })
      : retrying
        ? t("chat.activityGroup.retrying", { count: String(progress.retrying) })
        : running
          ? t("chat.activity.status.running")
          : failed
            ? t("chat.activityGroup.failed", { count: String(progress.error) })
            : declined
              ? t("chat.activityGroup.declined", {
                  count: String(progress.declined),
                })
              : interrupted
                ? t("chat.activityGroup.interrupted", {
                    count: String(progress.interrupted),
                  })
                : null,
  ].filter(Boolean);

  return (
    <section
      className={`msg-activity-group${open ? " is-open" : ""}${
        running ? " is-running" : ""
      }${retrying ? " is-retrying" : ""}${waiting ? " is-waiting" : ""}${
        failed ? " is-error" : ""
      }${partial ? " is-partial" : ""}${declined ? " is-declined" : ""}${interrupted ? " is-interrupted" : ""}`}
    >
      <button
        type="button"
        className="msg-activity-group-toggle"
        aria-expanded={open}
        aria-label={
          open
            ? t("chat.activityGroup.collapse")
            : t("chat.activityGroup.expand")
        }
        onClick={() => setOpen((value) => !value)}
      >
        <span className="msg-activity-group-icon" aria-hidden>
          <Layers3 size={14} strokeWidth={2} />
        </span>
        <span className="msg-activity-group-title">{actionLabel}</span>
        <span className="msg-activity-group-count">{metadata.join(" · ")}</span>
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
                forcedOpen ??
                (activity.status === "waiting" ||
                  activity.status === "error" ||
                  activity.status === "declined" ||
                  activity.status === "interrupted")
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
