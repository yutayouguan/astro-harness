/** 助手气泡内时间线：左侧轴线串联思考 / 工具 / hook / 回复等。 */
import type { ReactNode } from "react";
import {
  Activity,
  LayoutPanelTop,
  Layers3,
  Lightbulb,
  LoaderCircle,
  MessageCircle,
  Webhook,
} from "lucide-react";
import type { ChatActivity, ChatActivityKind } from "../../types";
import McpIcon from "../icons/McpIcon";
import { IconMemory, IconSkills, IconTools } from "../icons/NavIcons";
import { ActivityIcon } from "./MsgActivity";

export type MsgTimelineKind =
  | "reasoning"
  | ChatActivityKind
  | "surface"
  | "reply"
  | "generating";

type StepProps = {
  kind: MsgTimelineKind;
  /** 活动步骤的具体活动：轨道图标直接用行内那套细分图标（读取/运行/浏览…）。 */
  activity?: ChatActivity;
  /** 连续活动合并成的组：轨道改用「一组工具」图标。 */
  grouped?: boolean;
  /** 当前步骤仍在进行（思考中 / 工具 running / 生成中） */
  active?: boolean;
  isLast?: boolean;
  children: ReactNode;
};

function StepIcon({
  kind,
  activity,
  grouped = false,
}: {
  kind: MsgTimelineKind;
  activity?: ChatActivity;
  grouped?: boolean;
}) {
  // 时间线节点是唯一的图标位：优先展示最具体的那个，避免行内重复一个图标。
  if (grouped) return <Layers3 size={12} strokeWidth={2} aria-hidden />;
  if (activity) return <ActivityIcon activity={activity} size={12} />;
  const props = { size: 12, strokeWidth: 2.25, "aria-hidden": true as const };
  switch (kind) {
    case "reasoning":
      return <Lightbulb {...props} />;
    case "tool":
      return <IconTools width={12} height={12} />;
    case "skill":
      return <IconSkills width={12} height={12} />;
    case "mcp":
      return <McpIcon size={12} />;
    case "hook":
      return <Webhook {...props} />;
    case "memory":
      return <IconMemory width={12} height={12} />;
    case "status":
      return <Activity {...props} />;
    case "surface":
      return <LayoutPanelTop {...props} />;
    case "generating":
      return <LoaderCircle {...props} className="msg-timeline-spin" />;
    case "reply":
      return <MessageCircle {...props} />;
  }
}

export function MsgTimeline({ children }: { children: ReactNode }) {
  return <ol className="msg-timeline">{children}</ol>;
}

export function MsgTimelineStep({
  kind,
  activity,
  grouped = false,
  active = false,
  isLast = false,
  children,
}: StepProps) {
  return (
    <li
      className={`msg-timeline-step kind-${kind}${active ? " is-active" : ""}${
        isLast ? " is-last" : ""
      }`}
    >
      <span className="msg-timeline-rail" aria-hidden>
        <span className="msg-timeline-dot">
          <StepIcon kind={kind} activity={activity} grouped={grouped} />
        </span>
        {!isLast ? <span className="msg-timeline-line" /> : null}
      </span>
      <div className="msg-timeline-body">{children}</div>
    </li>
  );
}
