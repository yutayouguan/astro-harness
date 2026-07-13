/** 气泡内活动列表父折叠。 */
import { useEffect, useState } from "react";
import { ChevronDown, Wrench } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import type { ChatActivity } from "../types";
import MsgActivity from "./MsgActivity";

type Props = {
  items: ChatActivity[];
  /** verbosity === "detailed" 时为 true */
  defaultOpen: boolean;
  showTimestamp: boolean;
};

export default function ActivityGroup({
  items,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const [open, setOpen] = useState(defaultOpen);

  useEffect(() => {
    setOpen(defaultOpen);
  }, [defaultOpen]);

  if (!items.length) return null;

  return (
    <div className={`msg-activity-group ${open ? "is-open" : ""}`}>
      <button
        type="button"
        className="msg-activity-group-toggle"
        aria-expanded={open}
        aria-label={
          open ? t("chat.activityGroupCollapse") : t("chat.activityGroupExpand")
        }
        onClick={() => setOpen((v) => !v)}
      >
        <Wrench size={14} strokeWidth={2} className="msg-activity-group-icon" aria-hidden />
        <span className="msg-activity-group-label">
          {t("chat.activityGroup", { n: String(items.length) })}
        </span>
        <ChevronDown
          size={14}
          strokeWidth={2}
          className="msg-activity-group-chevron"
          aria-hidden
        />
      </button>
      {open ? (
        <div className="msg-activities">
          {items.map((a) => (
            <MsgActivity
              key={a.id}
              activity={a}
              defaultOpen={false}
              showTimestamp={showTimestamp}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}
