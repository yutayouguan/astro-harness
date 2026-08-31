/** 单条聊天活动卡：kind 图标 + 可折叠 IO / 生成媒体。 */
import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  Activity,
  BookOpenText,
  Globe2,
  Image as ImageIcon,
  PencilLine,
  Search,
  SquareTerminal,
  Webhook,
  Wrench,
} from "lucide-react";
import {
  ChevronDown as ChevronDownData,
  ChevronUp as ChevronUpData,
} from "lucide";
import { useI18n } from "../../i18n/LocaleContext";
import { useLiveElapsedSec } from "../../hooks/chat/useLiveElapsedSec";
import {
  activityHasBody,
  resolveActivityIO,
} from "../../lib/chat/resolveActivityIO";
import { formatElapsedSec } from "../../lib/chat/elapsedSec";
import { isLiveActivityStatus } from "../../lib/chat/toolActivityStatus";
import { parseGeneratedMedia } from "../../lib/media/parseGeneratedMedia";
import {
  looksLikeRelativeLocalPath,
  resolveMediaPreviewPath,
} from "../../lib/media/resolveMediaSrc";
import type { ChatActivity } from "../../types";
import {
  activityTitlePresentation,
  activityVisualKind,
} from "../../lib/chat/activityPresentation";
import McpIcon from "../icons/McpIcon";
import GeneratedMediaCard from "../media/GeneratedMediaCard";
import { IconMemory, IconSkills } from "../icons/NavIcons";
import { MorphToggleIcon } from "../icons/MorphIcon";
import { ChatMarkdown } from "./ChatMarkdown";

type Props = {
  activity: ChatActivity;
  defaultOpen: boolean;
  showTimestamp: boolean;
  mediaBaseDir?: string | null;
};

export function ActivityIcon({
  activity,
  size = 14,
}: {
  activity: ChatActivity;
  size?: number;
}) {
  const props = { size, strokeWidth: 2, "aria-hidden": true as const };
  switch (activityVisualKind(activity)) {
    case "read":
      return <BookOpenText {...props} />;
    case "search":
      return <Search {...props} />;
    case "run":
      return <SquareTerminal {...props} />;
    case "edit":
      return <PencilLine {...props} />;
    case "browse":
      return <Globe2 {...props} />;
    case "media":
      return <ImageIcon {...props} />;
    case "mcp":
      return <McpIcon size={size} />;
    case "skill":
      return <IconSkills width={size} height={size} />;
    case "memory":
      return <IconMemory width={size} height={size} />;
    case "hook":
      return <Webhook {...props} />;
    case "status":
      return <Activity {...props} />;
    case "tool":
      return <Wrench {...props} />;
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
    if (isLiveActivityStatus(activity.status)) return [];
    if (activity.media && activity.media.length > 0) return activity.media;
    return parseGeneratedMedia(output);
  }, [activity.status, activity.media, output]);
  // 相对路径需等 workspace 根；否则首屏会用不可加载 path 挂载预览
  const previewMedia = useMemo(
    () =>
      mediaItems
        .filter(
          (m) =>
            !looksLikeRelativeLocalPath(m.path) ||
            Boolean(mediaBaseDir?.trim()),
        )
        .map((m) => ({
          ...m,
          path: resolveMediaPreviewPath(m.path, mediaBaseDir),
        })),
    [mediaItems, mediaBaseDir],
  );
  const hasMedia = previewMedia.length > 0;
  const canToggle = hasBody || hasMedia;
  const [open, setOpen] = useState(() => defaultOpen && canToggle);
  const [inputOpen, setInputOpen] = useState(false);
  const running = isLiveActivityStatus(activity.status);
  const liveSec = useLiveElapsedSec(running, activity.at ?? null);

  useEffect(() => {
    if (canToggle) setOpen(defaultOpen);
  }, [defaultOpen, canToggle]);

  const statusClass = activity.status ? `is-${activity.status}` : "";
  const openClass = open ? "is-open" : "";
  const displayTitle = resolveActivityTitle(activity, t);

  const durationLabel = running
    ? liveSec != null
      ? t("chat.activityDuration", { s: formatElapsedSec(liveSec) })
      : t("chat.activity.status.running")
    : activity.durationSec != null
      ? t("chat.activityDuration", {
          s: formatElapsedSec(activity.durationSec),
        })
      : null;

  const metaLabel =
    showTimestamp && activity.at ? (
      <span className="msg-activity-time">
        {new Date(activity.at).toLocaleTimeString()}
      </span>
    ) : null;

  const summary: ReactNode = (
    <>
      <span className="msg-activity-kind-icon">
        <ActivityIcon activity={activity} />
      </span>
      <span className="msg-activity-title" title={activity.title}>
        {displayTitle}
      </span>
      {metaLabel || durationLabel ? (
        <span className="msg-activity-meta">
          {metaLabel}
          {durationLabel ? (
            <span className="msg-activity-duration">{durationLabel}</span>
          ) : null}
        </span>
      ) : null}
    </>
  );

  return (
    <div
      className={`msg-activity ${statusClass} ${openClass}`.trim()}
      data-kind={activity.kind}
    >
      <div className="msg-activity-body">
        {canToggle ? (
          <button
            type="button"
            className="msg-activity-toggle"
            aria-expanded={open}
            aria-label={`${displayTitle}，${open ? t("chat.activityCollapse") : t("chat.activityExpand")}`}
            onClick={() => setOpen((v) => !v)}
          >
            {summary}
            <MorphToggleIcon
              active={open}
              activeIcon={ChevronUpData}
              inactiveIcon={ChevronDownData}
              size={14}
              strokeWidth={2}
              className="msg-activity-chevron"
              aria-hidden
            />
          </button>
        ) : (
          <div className="msg-activity-summary">{summary}</div>
        )}
        {hasMedia || input || output ? (
          <div className="msg-activity-collapse">
            <div className="msg-activity-collapse-inner">
              {hasMedia ? (
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
              {input || output ? (
                <div className="msg-activity-io">
                  {input ? (
                    <div className="msg-activity-io-block is-input">
                      <button
                        type="button"
                        className="msg-activity-io-disclosure"
                        aria-expanded={inputOpen}
                        onClick={() => setInputOpen((value) => !value)}
                      >
                        <span className="msg-activity-io-label">
                          {t("chat.activityInput")}
                        </span>
                        <MorphToggleIcon
                          active={inputOpen}
                          activeIcon={ChevronUpData}
                          inactiveIcon={ChevronDownData}
                          size={13}
                          strokeWidth={1.8}
                          className="msg-activity-io-chevron"
                          aria-hidden
                        />
                      </button>
                      <div
                        className={`msg-activity-input-collapse${inputOpen ? " is-open" : ""}`}
                      >
                        <div className="msg-activity-input-collapse-inner">
                          <div className="msg-activity-detail is-input">
                            <ChatMarkdown
                              content={input}
                              compact
                              mediaBaseDir={mediaBaseDir}
                            />
                          </div>
                        </div>
                      </div>
                    </div>
                  ) : null}
                  {output ? (
                    <div className="msg-activity-io-block">
                      <span className="msg-activity-io-label">
                        {t("chat.activityOutput")}
                      </span>
                      <div className="msg-activity-detail is-output">
                        <ChatMarkdown
                          content={output}
                          compact
                          mediaBaseDir={mediaBaseDir}
                        />
                      </div>
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

function resolveActivityTitle(
  activity: ChatActivity,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const presentation = activityTitlePresentation(activity);
  if (!presentation) return activity.title;
  return presentation.target
    ? t(presentation.key, { target: presentation.target })
    : t(presentation.key);
}
