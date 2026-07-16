/** 助手气泡内时间线：左侧轴线串联思考 / 工具 / hook / 回复等。 */
import type { ReactNode } from "react";
import {
  Activity,
  LayoutPanelTop,
  Lightbulb,
  LoaderCircle,
  MessageCircle,
  Webhook,
} from "lucide-react";
import type { ChatActivityKind } from "../../types";
import McpIcon from "../icons/McpIcon";
import { IconMemory, IconSkills, IconTools } from "../icons/NavIcons";

export type MsgTimelineKind =
  | "reasoning"
  | ChatActivityKind
  | "surface"
  | "reply"
  | "generating";

type StepProps = {
  kind: MsgTimelineKind;
  /** 当前步骤仍在进行（思考中 / 工具 running / 生成中） */
  active?: boolean;
  isLast?: boolean;
  children: ReactNode;
};

function StepIcon({ kind }: { kind: MsgTimelineKind }) {
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
          <StepIcon kind={kind} />
        </span>
        {!isLast ? <span className="msg-timeline-line" /> : null}
      </span>
      <div className="msg-timeline-body">{children}</div>
    </li>
  );
}
