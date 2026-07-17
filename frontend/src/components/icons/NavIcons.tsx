/** 侧栏导航图标。 */
import {
  Children,
  cloneElement,
  isValidElement,
  useId,
  type CSSProperties,
  type ReactElement,
  type ReactNode,
  type SVGProps,
} from "react";

type IconProps = SVGProps<SVGSVGElement>;

type PaintedProps = {
  className?: string;
  fill?: string;
  stroke?: string;
  strokeWidth?: number | string;
  children?: ReactNode;
};

/** 给内部细节补上与外轮廓相同的渐变 stroke/fill 表现属性（WebKit 对 CSS url() 不可靠） */
function paintDetailChildren(children: ReactNode, paint: string): ReactNode {
  return Children.map(children, (child) => {
    if (!isValidElement<PaintedProps>(child)) return child;

    const cls = typeof child.props.className === "string" ? child.props.className : "";
    const nextChildren =
      child.props.children != null ? paintDetailChildren(child.props.children, paint) : child.props.children;

    if (cls.includes("nav-icon-cutout") || cls.includes("nav-icon-hole") || cls.includes("nav-icon-stroke")) {
      return cloneElement(child as ReactElement<PaintedProps>, {
        fill: "none",
        stroke: paint,
        children: nextChildren,
      });
    }

    if (cls.includes("nav-icon-dot")) {
      return cloneElement(child as ReactElement<PaintedProps>, {
        fill: paint,
        stroke: "none",
        children: nextChildren,
      });
    }

    if (cls.includes("nav-icon-fill")) {
      // 默认只描边；选中时由 CSS fill: inherit 填实
      return cloneElement(child as ReactElement<PaintedProps>, {
        fill: "none",
        stroke: paint,
        children: nextChildren,
      });
    }

    if (nextChildren !== child.props.children) {
      return cloneElement(child as ReactElement<PaintedProps>, { children: nextChildren });
    }

    return child;
  });
}

function IconBase({ children, style, ...props }: IconProps) {
  const rawId = useId();
  const gradId = `nav-icon-grad-${rawId.replace(/:/g, "")}`;
  const paint = `url(#${gradId})`;
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
      style={{ ["--nav-grad-paint" as string]: paint, ...style } as CSSProperties}
      fill="none"
      stroke={paint}
    >
      <defs>
        <linearGradient id={gradId} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--nav-grad-top, currentColor)" />
          <stop offset="100%" stopColor="var(--nav-grad-bottom, currentColor)" />
        </linearGradient>
      </defs>
      {paintDetailChildren(children, paint)}
    </svg>
  );
}

/** 智能对话 — 默认空心+三点线稿；选中填实后三点变透镜镂空 */
export function IconChat(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M7.9 20A9 9 0 1 0 4 16.1L2 22Z" />
      <path className="nav-icon-cutout" d="M8 12h.01" />
      <path className="nav-icon-cutout" d="M12 12h.01" />
      <path className="nav-icon-cutout" d="M16 12h.01" />
    </IconBase>
  );
}

/** 记忆空间 — 行星本体可填；光环始终只描边 */
export function IconMemory(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle className="nav-icon-fill" cx="12" cy="12" r="6.5" />
      <path
        className="nav-icon-stroke"
        d="M18.816 13.58c2.292 2.138 3.546 4 3.092 4.9-.745 1.46-5.783-.259-11.255-3.838-5.47-3.579-9.304-7.664-8.56-9.123.464-.91 2.926-.444 5.803.805"
      />
    </IconBase>
  );
}

/** 工作空间 — 默认空心+<> 线稿；选中填实后 <> 变透镜镂空 */
export function IconWorkspace(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2z" />
      <path className="nav-icon-cutout" d="M10 10.5 8 13l2 2.5" />
      <path className="nav-icon-cutout" d="M14 10.5 16 13l-2 2.5" />
    </IconBase>
  );
}

/** 文件空间 — 文档+折角+三横线；默认空心线稿，选中填实后细节镂空 */
export function IconFileSpace(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" />
      <path className="nav-icon-cutout" d="M14 2v4a2 2 0 0 0 2 2h4" />
      <path className="nav-icon-cutout" d="M8 9h3" />
      <path className="nav-icon-cutout" d="M8 13h8" />
      <path className="nav-icon-cutout" d="M8 17h8" />
    </IconBase>
  );
}

/** 我的工具 — 单一扳手剪影，选中整块填实，无内部镂空 */
export function IconTools(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.106-3.105c.32-.322.863-.22.983.218a6 6 0 0 1-8.259 7.057l-7.91 7.91a1 1 0 0 1-2.999-3l7.91-7.91a6 6 0 0 1 7.057-8.259c.438.12.54.662.219.984z" />
    </IconBase>
  );
}

/** 我的技能 — 星形剪影，选中整块填实，无内部镂空 */
export function IconSkills(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M12 3l1.5 4.5L18 9l-4.5 1.5L12 15l-1.5-4.5L6 9l4.5-1.5z" />
      <path d="M5 17l.75 2.25L8 20l-2.25.75L5 23l-.75-2.25L2 20l2.25-.75z" />
      <path d="M19 14l.5 1.5L21 16l-1.5.5L19 18l-.5-1.5L17 16l1.5-.5z" />
    </IconBase>
  );
}

/** 模型 / 性能 */
export function IconZap(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
    </IconBase>
  );
}

/** 安全 / 会话 */
export function IconShield(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z" />
    </IconBase>
  );
}

/** 液态玻璃 / 装饰 — 主星可填；小圆点为装饰实心 */
export function IconSparkles(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594z" />
      <path className="nav-icon-stroke" d="M20 2v4m2-2h-4" />
      <circle className="nav-icon-dot" cx="4" cy="20" r="2" />
    </IconBase>
  );
}

/** 定时任务 — 默认空心+勾线稿；选中表盘填实，勾变透镜镂空 */
export function IconCron(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="13" r="8" />
      <path className="nav-icon-stroke" d="M9 3.2 7.2 5.6" />
      <path className="nav-icon-stroke" d="M15 3.2 16.8 5.6" />
      <path className="nav-icon-cutout" d="m9.2 13.1 1.9 1.9 3.8-4" />
    </IconBase>
  );
}

/** 数据洞察 — 趋势箭头剪影，选中可整体填实 */
export function IconInsights(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4.4 19.1 3 17.7l6.6-6.6 3.2 3.2 5.8-5.8H16V6h6v6h-2.5V9.9l-6.7 6.7-3.2-3.2z" />
    </IconBase>
  );
}

/** 模型提供商 — 闪电剪影，选中整块填实 */
export function IconProviders(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z" />
    </IconBase>
  );
}

/** 偏好设置 — 默认空心+中心圆线稿；选中填实后中心变透镜孔 */
export function IconSettings(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0a2.34 2.34 0 0 0 3.319 1.915a2.34 2.34 0 0 1 2.33 4.033a2.34 2.34 0 0 0 0 3.831a2.34 2.34 0 0 1-2.33 4.033a2.34 2.34 0 0 0-3.319 1.915a2.34 2.34 0 0 1-4.659 0a2.34 2.34 0 0 0-3.32-1.915a2.34 2.34 0 0 1-2.33-4.033a2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" />
      <circle className="nav-icon-hole" cx="12" cy="12" r="3" />
    </IconBase>
  );
}

export function IconSun(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32l1.41 1.41M2 12h2m16 0h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
    </IconBase>
  );
}

export function IconMoon(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401" />
    </IconBase>
  );
}

export function IconMonitor(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect width="20" height="14" x="2" y="3" rx="2" />
      <path d="M8 21h8m-4-4v4" />
    </IconBase>
  );
}

/** 关于 Astro / 阿童木 — 核可填；轨道始终只描边 */
export function IconAtom(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle className="nav-icon-fill" cx="12" cy="12" r="1.5" />
      <path
        className="nav-icon-stroke"
        d="M20.2 20.2c2.04-2.03.02-7.36-4.5-11.9-4.54-4.52-9.87-6.54-11.9-4.5-2.04 2.03-.02 7.36 4.5 11.9 4.54 4.52 9.87 6.54 11.9 4.5"
      />
      <path
        className="nav-icon-stroke"
        d="M15.7 15.7c4.52-4.54 6.54-9.87 4.5-11.9-2.03-2.04-7.36-.02-11.9 4.5-4.52 4.54-6.54 9.87-4.5 11.9 2.03 2.04 7.36.02 11.9-4.5"
      />
    </IconBase>
  );
}

/** 语言 / 地球 */
export function IconGlobe(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 2a14.5 14.5 0 0 0 0 20a14.5 14.5 0 0 0 0-20" />
      <path d="M2 12h20" />
    </IconBase>
  );
}

/** 展开侧栏 */
export function IconPanelOpen(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18m5-12l3 3l-3 3" />
    </IconBase>
  );
}

/** 收起侧栏 */
export function IconPanelClose(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18m7-6l-3-3l3-3" />
    </IconBase>
  );
}

/** 侧栏显示文字（宽） */
export function IconSidebarLabels(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18" />
      <path d="M13 8h5M13 12h5M13 16h3" />
    </IconBase>
  );
}

/** 侧栏仅图标（窄） */
export function IconSidebarIcons(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18" />
    </IconBase>
  );
}

/** 展开右侧伴随栏 */
export function IconRightPanel(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="3" y="3" width="18" height="18" rx="3" />
      <path d="M15 3v18" />
      <path d="m8 9 3 3-3 3" />
    </IconBase>
  );
}

/** 展开对话（四角外扩） */
export function IconExpand(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 3H5a2 2 0 0 0-2 2v3" />
      <path d="M21 8V5a2 2 0 0 0-2-2h-3" />
      <path d="M3 16v3a2 2 0 0 0 2 2h3" />
      <path d="M16 21h3a2 2 0 0 0 2-2v-3" />
    </IconBase>
  );
}

/** 收起对话（四角内收） */
export function IconCollapse(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M8 3v3a2 2 0 0 1-2 2H3" />
      <path d="M21 8h-3a2 2 0 0 1-2-2V3" />
      <path d="M3 16h3a2 2 0 0 1 2 2v3" />
      <path d="M16 21v-3a2 2 0 0 1 2-2h3" />
    </IconBase>
  );
}

/** 新建会话 */
export function IconNewChat(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
      <path d="M12 7v6" />
      <path d="M9 10h6" />
    </IconBase>
  );
}

/** 新建 Agent */
export function IconNewSession(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M2.992 16.342a2 2 0 0 1 .094 1.167l-1.065 3.29a1 1 0 0 0 1.236 1.168l3.413-.998a2 2 0 0 1 1.099.092a10 10 0 1 0-4.777-4.719" />
      <path d="M12 8v6" />
      <path d="M9 11h6" />
    </IconBase>
  );
}

/** 搜索 */
export function IconSearch(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="11" cy="11" r="8" />
      <path d="m21 21-4.3-4.3" />
    </IconBase>
  );
}

/** 刷新 */
export function IconRefresh(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
      <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
      <path d="M8 16H3v5" />
    </IconBase>
  );
}
