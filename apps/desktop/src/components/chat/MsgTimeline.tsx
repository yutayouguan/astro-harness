/** 助手气泡内时间线：左侧轴线串联思考 / 工具 / hook / 回复等。 */
import type { ReactNode } from "react";
import type { ChatActivityKind } from "../../types";

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

export function MsgTimeline({ children }: { children: ReactNode }) {
  return <ol className="msg-timeline">{children}</ol>;
}

/**
 * 时间线节点只保留分类色点。
 *
 * 图标一律由行内图标位承担（思考 💡、连续工具调用成组用组图标、单独调用用各自的
 * 细分图标）；节点再画一个图标就会与行内重复，语义还会互相打架。
 */
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
        <span className="msg-timeline-dot" />
        {!isLast ? <span className="msg-timeline-line" /> : null}
      </span>
      <div className="msg-timeline-body">{children}</div>
    </li>
  );
}
