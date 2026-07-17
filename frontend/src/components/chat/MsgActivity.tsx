/** 单条聊天活动卡：kind 图标 + 可折叠 IO；生成媒体以精美卡片始终露出。 */
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { Activity, ChevronDown, Webhook } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useLiveElapsedSec } from "../../hooks/chat/useLiveElapsedSec";
import {
  activityHasBody,
  resolveActivityIO,
} from "../../lib/chat/resolveActivityIO";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";
import { parseGeneratedMedia } from "../../lib/media/parseGeneratedMedia";
import {
  looksLikeRelativeLocalPath,
  resolveMediaPreviewPath,
} from "../../lib/media/resolveMediaSrc";
import type { ChatActivity, ChatActivityKind } from "../../types";
import McpIcon from "../icons/McpIcon";
import GeneratedMediaCard from "../media/GeneratedMediaCard";
import { IconMemory, IconSkills, IconTools } from "../icons/NavIcons";

type Props = {
  activity: ChatActivity;
  defaultOpen: boolean;
  showTimestamp: boolean;
  mediaBaseDir?: string | null;
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
  mediaBaseDir,
}: Props) {
  const { t } = useI18n();
  const hasBody = activityHasBody(activity);
  const { input, output } = resolveActivityIO(activity);
  const mediaItems = useMemo(() => {
    if (activity.status === "running") return [];
    if (activity.media && activity.media.length > 0) return activity.media;
    return parseGeneratedMedia(output);
  }, [activity.status, activity.media, output]);
  // 相对路径需等 workspace 根；否则首屏会用不可加载 path 挂载预览
  const previewMedia = useMemo(
    () =>
      mediaItems
        .filter(
          (m) =>
            !looksLikeRelativeLocalPath(m.path) || Boolean(mediaBaseDir?.trim()),
        )
        .map((m) => ({
          ...m,
          path: resolveMediaPreviewPath(m.path, mediaBaseDir),
        })),
    [mediaItems, mediaBaseDir],
  );
  const [open, setOpen] = useState(() => defaultOpen && hasBody);
  const running = activity.status === "running";
  const liveSec = useLiveElapsedSec(running, activity.at ?? null);

  useEffect(() => {
    if (hasBody) setOpen(defaultOpen);
  }, [defaultOpen, hasBody]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";

  const durationLabel = running
    ? liveSec != null
      ? t("chat.activityDuration", { s: formatElapsedSec(liveSec) })
      : t("chat.activity.status.running")
    : activity.durationSec != null
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
        {/* 生成媒体始终露出，不随 Input/Output 折叠 */}
        {previewMedia.length > 0 ? (
          <div className="msg-activity-media">
            {previewMedia.map((m) => (
              <GeneratedMediaCard
                key={`${m.kind}:${m.path}`}
                kind={m.kind}
                path={m.path}
                compact
              />
            ))}
          </div>
        ) : null}
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
