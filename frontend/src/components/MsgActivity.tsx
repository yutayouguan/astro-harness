/** 单条聊天活动卡：kind 图标 + 可折叠 Input/Output。 */
import { useEffect, useState, type ReactNode } from "react";
import { Activity, ChevronDown, Webhook } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import {
  activityHasBody,
  resolveActivityIO,
} from "../lib/resolveActivityIO";
import type { ChatActivity, ChatActivityKind } from "../types";
import McpIcon from "./McpIcon";
import { IconMemory, IconSkills, IconTools } from "./NavIcons";

type Props = {
  activity: ChatActivity;
  defaultOpen: boolean;
  showTimestamp: boolean;
};

function KindIcon({ kind }: { kind: ChatActivityKind }) {
  switch (kind) {
    case "mcp":
      return <McpIcon size={14} />;
    case "tool":
      return <IconTools width={14} height={14} />;
    case "skill":
      return <IconSkills width={14} height={14} />;
    case "memory":
      return <IconMemory width={14} height={14} />;
    case "hook":
      return <Webhook size={14} strokeWidth={2} aria-hidden />;
    case "status":
      return <Activity size={14} strokeWidth={2} aria-hidden />;
  }
}

export default function MsgActivity({
  activity,
  defaultOpen,
  showTimestamp,
}: Props) {
  const { t } = useI18n();
  const hasBody = activityHasBody(activity);
  const { input, output } = resolveActivityIO(activity);
  const [open, setOpen] = useState(defaultOpen && hasBody);

  useEffect(() => {
    if (hasBody) setOpen(defaultOpen);
  }, [defaultOpen, hasBody]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  const summary: ReactNode = (
    <>
      <span className="msg-activity-kind-icon">
        <KindIcon kind={activity.kind} />
      </span>
      <span className="msg-activity-title">{activity.title}</span>
    </>
  );

  return (
    <div
      className={`msg-activity ${statusClass} ${openClass}`.trim()}
      data-kind={activity.kind}
    >
      <div className="msg-activity-body">
        {hasBody ? (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            aria-label={`${activity.title}，${open ? t("chat.activityCollapse") : t("chat.activityExpand")}`}
            onClick={() => setOpen((v) => !v)}
          >
            {summary}
            <ChevronDown
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        ) : (
          <div className="msg-activity-summary">{summary}</div>
        )}
        {open && (input || output) ? (
          <div className="msg-activity-io">
            {input ? (
              <div className="msg-activity-io-block">
                <span className="msg-activity-io-label">
                  {t("chat.activityInput")}
                </span>
                <pre className="msg-activity-detail">{input}</pre>
              </div>
            ) : null}
            {output ? (
              <div className="msg-activity-io-block">
                <span className="msg-activity-io-label">
                  {t("chat.activityOutput")}
                </span>
                <pre className="msg-activity-detail">{output}</pre>
              </div>
            ) : null}
          </div>
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
