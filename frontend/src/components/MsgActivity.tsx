/** 单条聊天活动卡：kind 图标 + 可折叠正文（生成媒体预览与 Input/Output）。 */
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Activity, ChevronDown, Webhook } from "lucide-react";
import { useI18n } from "../i18n/LocaleContext";
import { useLiveElapsedSec } from "../hooks/useLiveElapsedSec";
import {
  activityHasBody,
  resolveActivityIO,
} from "../lib/resolveActivityIO";
import { formatElapsedSec } from "../lib/elapsedSec";
import { parseGeneratedMedia } from "../lib/parseGeneratedMedia";
import type { ChatActivity, ChatActivityKind } from "../types";
import McpIcon from "./McpIcon";
import MediaPreview from "./media/MediaPreview";
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
  const mediaItems = useMemo(
    () =>
      activity.status === "running" ? [] : parseGeneratedMedia(output),
    [activity.status, output],
  );
  const [open, setOpen] = useState(
    () => (defaultOpen && hasBody) || mediaItems.length > 0,
  );
  const running = activity.status === "running";
  const liveSec = useLiveElapsedSec(running, activity.at ?? null);
  const prevMediaCountRef = useRef(mediaItems.length);

  useEffect(() => {
    if (hasBody && mediaItems.length === 0) setOpen(defaultOpen);
  }, [defaultOpen, hasBody, mediaItems.length]);

  // 生成刚完成（0 → N）时展开以便看到结果；之后尊重用户折叠，不再强行打开。
  useEffect(() => {
    const prev = prevMediaCountRef.current;
    prevMediaCountRef.current = mediaItems.length;
    if (prev === 0 && mediaItems.length > 0) setOpen(true);
  }, [mediaItems.length]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  const durationLabel = running
    ? liveSec != null
      ? t("chat.activityDuration", { s: formatElapsedSec(liveSec) })
      : t("chat.activity.status.running")
    : activity.durationSec != null && activity.durationSec > 0
      ? t("chat.activityDuration", {
          s: formatElapsedSec(activity.durationSec),
        })
      : null;

  const summary: ReactNode = (
    <>
      <span className="msg-activity-kind-icon">
        <KindIcon kind={activity.kind} />
      </span>
      <span className="msg-activity-title">{activity.title}</span>
      {durationLabel ? (
        <span className="msg-activity-duration">{durationLabel}</span>
      ) : null}
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
        {open ? (
          <>
            {mediaItems.length > 0 ? (
              <div className="msg-activity-media">
                {mediaItems.map((m) => (
                  <MediaPreview
                    key={`${m.kind}:${m.path}`}
                    kind={m.kind}
                    path={m.path}
                    compact
                  />
                ))}
              </div>
            ) : null}
            {input || output ? (
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
          </>
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
