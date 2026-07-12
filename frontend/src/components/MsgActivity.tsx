/** 单条聊天活动卡：摘要行 + 可折叠详情。 */
import { useEffect, useState } from "react";
import { ChevronDown } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { ChatActivity } from "../types";

type Props = {
  activity: ChatActivity;
  /** verbosity === "detailed" 时为 true */
  defaultOpen: boolean;
  showTimestamp: boolean;
};

export default function MsgActivity({
  activity,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const hasDetail = Boolean(activity.detail);
  const [open, setOpen] = useState(defaultOpen && hasDetail);

  useEffect(() => {
    if (hasDetail) setOpen(defaultOpen);
  }, [defaultOpen, hasDetail]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  return (
    <div
      className={`msg-activity ${statusClass} ${openClass}`.trim()}
      data-kind={activity.kind}
    >
      <span className="msg-activity-kind">{activity.kind}</span>
      <div className="msg-activity-body">
        {hasDetail ? (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            aria-label={open ? t("chat.activityCollapse") : t("chat.activityExpand")}
            onClick={() => setOpen((v) => !v)}
          >
            <span className="msg-activity-title">{activity.title}</span>
            <ChevronDown
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        ) : (
          <span className="msg-activity-title">{activity.title}</span>
        )}
        {open && activity.detail ? (
          <pre className="msg-activity-detail">{activity.detail}</pre>
        ) : null}
        {showTimestamp && activity.at ? (
          <span className="msg-activity-time">
            {new Date(activity.at).toLocaleTimeString()}
          </span>
        ) : null}
      </div>
    </div>
  );
}
