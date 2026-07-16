/** 工具相关图标组件集。 */
import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement>;

function IconBase({ children, ...props }: IconProps) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
      {...props}
    >
      {children}
    </svg>
  );
}

export function IconWebSearch(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M2 12h20M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z" />
    </IconBase>
  );
}

export function IconBrowser(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="M3 8h18" />
      <circle cx="7" cy="6" r="0.8" fill="currentColor" stroke="none" />
      <circle cx="10" cy="6" r="0.8" fill="currentColor" stroke="none" />
    </IconBase>
  );
}

export function IconTerminal(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="m4 17 6-6-6-6" />
      <path d="M12 19h8" />
    </IconBase>
  );
}

export function IconFileOps(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
      <path d="M14 2v6h6" />
      <path d="M8 13h8M8 17h5" />
    </IconBase>
  );
}

export function IconCodeExec(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="m16 18 6-6-6-6" />
      <path d="m8 6-6 6 6 6" />
    </IconBase>
  );
}

export function IconEye(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M2 12s3-7 10-7 10 7 10 7-3 7-10 7-10-7-10-7Z" />
      <circle cx="12" cy="12" r="3" />
    </IconBase>
  );
}

export function IconAudioUnderstand(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4 10v4" />
      <path d="M8 7v10" />
      <path d="M12 4v16" />
      <path d="M16 7v10" />
      <path d="M20 10v4" />
    </IconBase>
  );
}

export function IconRobotics(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M4 21h6" />
      <path d="M7 21v-4" />
      <circle cx="7" cy="15" r="1.5" />
      <path d="M7 15 11 10" />
      <circle cx="11" cy="10" r="1.5" />
      <path d="M11 10 18 6" />
      <circle cx="18" cy="6" r="1.5" />
      <path d="M18 6 21 3" />
      <path d="M18 6 21 9" />
    </IconBase>
  );
}

export function IconImageGen(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="3" y="5" width="18" height="14" rx="2" />
      <circle cx="8.5" cy="10.5" r="1.5" />
      <path d="m21 17-5-5L5 19" />
    </IconBase>
  );
}

export function IconVideoGen(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="2" y="6" width="14" height="12" rx="2" />
      <path d="m16 10 6-3v10l-6-3v-4z" />
    </IconBase>
  );
}

export function IconTts(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M11 5 6 9H3v6h3l5 4V5z" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" />
    </IconBase>
  );
}

export function IconSkillsTool(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M12 2 2 7l10 5 10-5-10-5z" />
      <path d="m2 17 10 5 10-5" />
      <path d="m2 12 10 5 10-5" />
    </IconBase>
  );
}

export function IconMemoryTool(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z" />
      <path d="M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z" />
      <path d="M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4" />
    </IconBase>
  );
}

export function IconSessionSearch(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
      <path d="M11 8v6M8 11h6" />
    </IconBase>
  );
}

export function IconClarify(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <path d="M12 17h.01" />
    </IconBase>
  );
}

export function IconConfirm(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M9 11 12 14 22 4" />
      <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
    </IconBase>
  );
}

export function IconLocation(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M12 21s7-4.5 7-11a7 7 0 1 0-14 0c0 6.5 7 11 7 11z" />
      <circle cx="12" cy="10" r="2.5" />
    </IconBase>
  );
}

export function IconPresentUi(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="3" y="3" width="18" height="18" rx="2" />
      <path d="M3 9h18" />
      <path d="M9 21V9" />
    </IconBase>
  );
}

export function IconDelegate(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87" />
      <path d="M16 3.13a4 4 0 0 1 0 7.75" />
    </IconBase>
  );
}

export function IconScheduled(props: IconProps) {
  return (
    <IconBase {...props}>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 6v6l4 2" />
    </IconBase>
  );
}

export function IconMultiAgent(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M12 2 2 7l10 5 10-5-10-5z" />
      <path d="M2 17l10 5 10-5" />
      <path d="M2 12l10 5 10-5" />
    </IconBase>
  );
}

export function IconTaskPlan(props: IconProps) {
  return (
    <IconBase {...props}>
      <rect x="3" y="5" width="18" height="16" rx="2" />
      <path d="M8 10h8M8 14h5" />
      <path d="m9 7 1.5 1.5L13 6" />
    </IconBase>
  );
}

export function IconMusic(props: IconProps) {
  return (
    <IconBase {...props}>
      <path d="M9 18V5l12-2v13" />
      <circle cx="6" cy="18" r="3" />
      <circle cx="18" cy="16" r="3" />
    </IconBase>
  );
}
