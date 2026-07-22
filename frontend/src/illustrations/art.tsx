/**
 * 空状态 / Agent 封面插画 — 自绘扁平 SVG，主色跟 currentColor。
 */
import type { ReactNode, SVGProps } from "react";

export type IllustProps = SVGProps<SVGSVGElement> & {
  title?: string;
};

function frame(props: IllustProps, children: ReactNode) {
  const { title, className, ...rest } = props;
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 160 120"
      fill="none"
      className={className}
      role={title ? "img" : "presentation"}
      aria-hidden={title ? undefined : true}
      {...rest}
    >
      {title ? <title>{title}</title> : null}
      {children}
    </svg>
  );
}

/** 共享软地面 + 雾点 */
function ground() {
  return (
    <>
      <ellipse cx="80" cy="98" rx="54" ry="8" fill="currentColor" opacity="0.08" />
      <circle cx="28" cy="28" r="3" fill="currentColor" opacity="0.12" />
      <circle cx="138" cy="36" r="2.5" fill="currentColor" opacity="0.1" />
      <circle cx="122" cy="18" r="2" fill="currentColor" opacity="0.08" />
    </>
  );
}

/** 聊天空状态 */
export function IllustEmptyChat(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <rect x="38" y="28" width="84" height="58" rx="14" fill="currentColor" opacity="0.1" />
      <rect
        x="38"
        y="28"
        width="84"
        height="58"
        rx="14"
        stroke="currentColor"
        strokeWidth="2"
        opacity="0.55"
      />
      <circle cx="58" cy="52" r="6" fill="currentColor" opacity="0.35" />
      <path
        d="M70 46h36M70 56h28"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        opacity="0.45"
      />
      <path
        d="M72 86c8 10 22 10 30 0"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.35"
      />
      <circle cx="118" cy="40" r="10" fill="currentColor" opacity="0.18" />
      <path
        d="M114 40h8M118 36v8"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.65"
      />
    </>,
  );
}

/** 工作区空 */
export function IllustEmptyWorkspace(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <path
        d="M42 78V46a6 6 0 0 1 6-6h22l8 8h34a6 6 0 0 1 6 6v24a6 6 0 0 1-6 6H48a6 6 0 0 1-6-6Z"
        fill="currentColor"
        opacity="0.12"
        stroke="currentColor"
        strokeWidth="2"
      />
      <rect x="56" y="58" width="48" height="6" rx="3" fill="currentColor" opacity="0.28" />
      <rect x="56" y="68" width="32" height="6" rx="3" fill="currentColor" opacity="0.18" />
      <circle cx="118" cy="34" r="14" fill="currentColor" opacity="0.12" />
      <path
        d="M118 28v12M112 34h12"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        opacity="0.55"
      />
    </>,
  );
}

/** 记忆空 */
export function IllustEmptyMemory(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <path
        d="M80 26c-16 0-28 12-28 28 0 22 28 40 28 40s28-18 28-40c0-16-12-28-28-28Z"
        fill="currentColor"
        opacity="0.12"
        stroke="currentColor"
        strokeWidth="2"
      />
      <circle cx="80" cy="52" r="10" fill="currentColor" opacity="0.22" />
      <path
        d="M80 48v8M76 52h8"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.55"
      />
    </>,
  );
}

/** Cron 空 */
export function IllustEmptyCron(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <circle
        cx="80"
        cy="54"
        r="30"
        fill="currentColor"
        opacity="0.08"
        stroke="currentColor"
        strokeWidth="2"
      />
      <circle cx="80" cy="54" r="3" fill="currentColor" opacity="0.55" />
      <path
        d="M80 38v16l12 8"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.55"
      />
      <path
        d="M80 24v6M80 78v6M46 54h-6M120 54h-6"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.28"
      />
    </>,
  );
}

/** 文件空 */
export function IllustEmptyFiles(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <path
        d="M52 34h36l10 10v42a6 6 0 0 1-6 6H52a6 6 0 0 1-6-6V40a6 6 0 0 1 6-6Z"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path d="M88 34v12h12" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
      <rect x="60" y="56" width="28" height="4" rx="2" fill="currentColor" opacity="0.3" />
      <rect x="60" y="66" width="20" height="4" rx="2" fill="currentColor" opacity="0.2" />
      <rect
        x="96"
        y="48"
        width="28"
        height="36"
        rx="6"
        fill="currentColor"
        opacity="0.12"
        stroke="currentColor"
        strokeWidth="2"
      />
    </>,
  );
}

/** Providers 空 */
export function IllustEmptyProviders(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <rect
        x="40"
        y="36"
        width="80"
        height="48"
        rx="12"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <circle cx="62" cy="60" r="8" fill="currentColor" opacity="0.28" />
      <path
        d="M78 54h28M78 64h20"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        opacity="0.4"
      />
      <path
        d="M108 28l4 8 8 1-6 5 2 8-8-4-8 4 2-8-6-5 8-1 4-8Z"
        fill="currentColor"
        opacity="0.35"
      />
    </>,
  );
}

/** 技能空 */
export function IllustEmptySkills(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      <g transform="rotate(-7 62 62)">
        <rect
          x="34"
          y="34"
          width="58"
          height="58"
          rx="13"
          fill="currentColor"
          opacity="0.08"
          stroke="currentColor"
          strokeWidth="2"
        />
        <path
          d="M48 50h30M48 60h22M48 70h26"
          stroke="currentColor"
          strokeWidth="2.5"
          strokeLinecap="round"
          opacity="0.32"
        />
      </g>
      <g transform="rotate(7 96 58)">
        <rect
          x="70"
          y="27"
          width="58"
          height="58"
          rx="13"
          fill="currentColor"
          opacity="0.14"
          stroke="currentColor"
          strokeWidth="2"
        />
        <path
          d="M99 39l3.5 8 8.5 1-6.5 5.5 2 8.5-7.5-4.5-7.5 4.5 2-8.5L87 48l8.5-1 3.5-8Z"
          fill="currentColor"
          opacity="0.42"
        />
      </g>
      <circle cx="128" cy="76" r="12" fill="currentColor" opacity="0.12" />
      <path
        d="M128 70v12M122 76h12"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        opacity="0.55"
      />
    </>,
  );
}

/** Loop 空状态 — 火箭起飞 */
export function IllustEmptyLoop(props: IllustProps) {
  return frame(
    props,
    <>
      {ground()}
      {/* 火箭主体 */}
      <path
        d="M80 22c-6 10-10 24-10 38h20c0-14-4-28-10-38Z"
        fill="currentColor"
        opacity="0.12"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinejoin="round"
      />
      {/* 火箭窗 */}
      <circle cx="80" cy="44" r="5" fill="currentColor" opacity="0.28" />
      {/* 左翼 */}
      <path
        d="M70 60c-6 2-10 8-12 14h12Z"
        fill="currentColor"
        opacity="0.18"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      {/* 右翼 */}
      <path
        d="M90 60c6 2 10 8 12 14H90Z"
        fill="currentColor"
        opacity="0.18"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
      {/* 尾焰 */}
      <path
        d="M74 74c2 8 4 14 6 18 2-4 4-10 6-18"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.4"
      />
      <path
        d="M77 74c1 5 2 8 3 10 1-2 2-5 3-10"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        opacity="0.25"
      />
      {/* 星星装饰 */}
      <circle cx="50" cy="34" r="2" fill="currentColor" opacity="0.2" />
      <circle cx="114" cy="42" r="2.5" fill="currentColor" opacity="0.15" />
      <circle cx="106" cy="28" r="1.5" fill="currentColor" opacity="0.18" />
    </>,
  );
}

/** Agent 封面：通用助手 */
export function IllustCoverAssistant(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <circle cx="80" cy="48" r="22" fill="currentColor" opacity="0.18" />
      <circle cx="80" cy="48" r="14" fill="currentColor" opacity="0.28" />
      <path
        d="M52 96c4-18 16-28 28-28s24 10 28 28"
        fill="currentColor"
        opacity="0.2"
      />
      <circle cx="72" cy="46" r="2.5" fill="currentColor" opacity="0.7" />
      <circle cx="88" cy="46" r="2.5" fill="currentColor" opacity="0.7" />
      <path
        d="M74 56c3 3 9 3 12 0"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        opacity="0.55"
      />
    </>,
  );
}

/** 封面：代码 */
export function IllustCoverCode(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <rect
        x="36"
        y="30"
        width="88"
        height="60"
        rx="10"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M56 48l-10 12 10 12M104 48l10 12-10 12M86 44l-12 32"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.55"
      />
    </>,
  );
}

/** 封面：研究 */
export function IllustCoverResearch(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <circle
        cx="72"
        cy="50"
        r="22"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2.5"
      />
      <path
        d="M88 66l18 18"
        stroke="currentColor"
        strokeWidth="3.5"
        strokeLinecap="round"
        opacity="0.55"
      />
      <circle cx="72" cy="50" r="8" fill="currentColor" opacity="0.22" />
    </>,
  );
}

/** 封面：写作 */
export function IllustCoverWriting(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <rect
        x="46"
        y="28"
        width="68"
        height="64"
        rx="8"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M58 46h44M58 58h36M58 70h28"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        opacity="0.4"
      />
      <path
        d="M108 78l14-14 6 6-14 14h-6v-6Z"
        fill="currentColor"
        opacity="0.35"
      />
    </>,
  );
}

/** 封面：日程 */
export function IllustCoverSchedule(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <rect
        x="44"
        y="30"
        width="72"
        height="62"
        rx="10"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path d="M44 48h72" stroke="currentColor" strokeWidth="2" opacity="0.35" />
      <path
        d="M60 24v12M100 24v12"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        opacity="0.5"
      />
      <circle cx="64" cy="66" r="5" fill="currentColor" opacity="0.35" />
      <circle cx="80" cy="66" r="5" fill="currentColor" opacity="0.22" />
      <circle cx="96" cy="66" r="5" fill="currentColor" opacity="0.15" />
    </>,
  );
}

/** 封面：数据 */
export function IllustCoverData(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <path
        d="M48 84V52M72 84V40M96 84V60M120 84V34"
        stroke="currentColor"
        strokeWidth="8"
        strokeLinecap="round"
        opacity="0.28"
      />
      <path
        d="M48 52l24-12 24 20 24-26"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.55"
      />
    </>,
  );
}

/** 封面：创意 */
export function IllustCoverCreative(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <path
        d="M80 26c-2 18-18 28-18 42a18 18 0 0 0 36 0c0-14-16-24-18-42Z"
        fill="currentColor"
        opacity="0.22"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M72 90h16M74 98h12"
        stroke="currentColor"
        strokeWidth="2.5"
        strokeLinecap="round"
        opacity="0.4"
      />
      <circle cx="118" cy="38" r="4" fill="currentColor" opacity="0.35" />
      <circle cx="42" cy="44" r="3" fill="currentColor" opacity="0.25" />
    </>,
  );
}

/** 封面：旅行/探索 */
export function IllustCoverExplore(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <circle
        cx="80"
        cy="56"
        r="28"
        fill="currentColor"
        opacity="0.1"
        stroke="currentColor"
        strokeWidth="2"
      />
      <ellipse cx="80" cy="56" rx="12" ry="28" stroke="currentColor" strokeWidth="2" opacity="0.4" />
      <path d="M52 56h56M80 28v56" stroke="currentColor" strokeWidth="2" opacity="0.35" />
      <path
        d="M54 42c16 6 36 6 52 0M54 70c16-6 36-6 52 0"
        stroke="currentColor"
        strokeWidth="1.8"
        opacity="0.3"
      />
    </>,
  );
}

/** 封面：安全 */
export function IllustCoverShield(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <path
        d="M80 26l34 12v24c0 22-14 36-34 42-20-6-34-20-34-42V38l34-12Z"
        fill="currentColor"
        opacity="0.14"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M66 58l10 10 20-22"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.55"
      />
    </>,
  );
}

/** 封面：消息 */
export function IllustCoverChat(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <rect
        x="34"
        y="34"
        width="58"
        height="40"
        rx="12"
        fill="currentColor"
        opacity="0.18"
      />
      <path d="M46 74l8-8h-8v8Z" fill="currentColor" opacity="0.18" />
      <rect
        x="74"
        y="48"
        width="52"
        height="34"
        rx="12"
        fill="currentColor"
        opacity="0.28"
      />
      <path d="M114 82l-8-8h8v8Z" fill="currentColor" opacity="0.28" />
    </>,
  );
}

/** 封面：音乐 */
export function IllustCoverMusic(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <path
        d="M70 34v42"
        stroke="currentColor"
        strokeWidth="3"
        strokeLinecap="round"
        opacity="0.45"
      />
      <circle cx="58" cy="78" r="12" fill="currentColor" opacity="0.28" />
      <path
        d="M70 34c18 4 28 2 36-2v28c-8 4-18 6-36 2"
        fill="currentColor"
        opacity="0.18"
        stroke="currentColor"
        strokeWidth="2"
      />
    </>,
  );
}

/** 封面：星空 */
export function IllustCoverStars(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <path
        d="M80 28l6 16h17l-14 10 5 17-14-10-14 10 5-17-14-10h17l6-16Z"
        fill="currentColor"
        opacity="0.32"
      />
      <circle cx="40" cy="44" r="3" fill="currentColor" opacity="0.35" />
      <circle cx="124" cy="40" r="2.5" fill="currentColor" opacity="0.3" />
      <circle cx="118" cy="78" r="3.5" fill="currentColor" opacity="0.22" />
      <circle cx="48" cy="82" r="2" fill="currentColor" opacity="0.25" />
    </>,
  );
}

/** 封面：工具箱 */
export function IllustCoverToolkit(props: IllustProps) {
  return frame(
    props,
    <>
      <rect width="160" height="120" rx="16" fill="currentColor" opacity="0.08" />
      <rect
        x="40"
        y="48"
        width="80"
        height="40"
        rx="8"
        fill="currentColor"
        opacity="0.14"
        stroke="currentColor"
        strokeWidth="2"
      />
      <path
        d="M60 48V40a8 8 0 0 1 8-8h24a8 8 0 0 1 8 8v8"
        stroke="currentColor"
        strokeWidth="2.5"
        opacity="0.45"
      />
      <rect x="72" y="60" width="16" height="10" rx="3" fill="currentColor" opacity="0.35" />
    </>,
  );
}
