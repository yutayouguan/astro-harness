import { forwardRef, type ComponentProps, type ElementRef } from "react";
import { MorphIcon as BaseMorphIcon, type MorphHandle } from "morphicons/react";
import { Check as CheckData, Copy as CopyData } from "lucide";
import { useMorphicons } from "../../hooks/app/useMorphicons";
import {
  toMorphIconInput,
  type MorphableIcon,
} from "../../lib/ui/morphIconData";

export type { MorphableIcon } from "../../lib/ui/morphIconData";

export type AppMorphIconProps = Omit<
  ComponentProps<typeof BaseMorphIcon>,
  "icon" | "spring" | "strokeWidth" | "reducedMotion"
> & {
  icon: MorphableIcon;
  spring?: ComponentProps<typeof BaseMorphIcon>["spring"];
  strokeWidth?: number | string;
};

export const AppMorphIcon = forwardRef<MorphHandle, AppMorphIconProps>(
  function AppMorphIcon({ icon, spring, strokeWidth, ...props }, ref) {
    const prefs = useMorphicons();
    return (
      <BaseMorphIcon
        ref={ref}
        icon={toMorphIconInput(icon)}
        spring={spring ?? prefs.spring}
        strokeWidth={strokeWidth ?? prefs.strokeWidth}
        reducedMotion="user"
        {...props}
      />
    );
  },
);

export type MorphToggleIconProps = Omit<AppMorphIconProps, "icon"> & {
  active: boolean;
  activeIcon: MorphableIcon;
  inactiveIcon: MorphableIcon;
};

export function MorphToggleIcon({
  active,
  activeIcon,
  inactiveIcon,
  ...props
}: MorphToggleIconProps) {
  return <AppMorphIcon icon={active ? activeIcon : inactiveIcon} {...props} />;
}

export function CopyMorphIcon({
  copied,
  copiedStrokeWidth = 2.4,
  strokeWidth,
  ...props
}: Omit<AppMorphIconProps, "icon"> & {
  copied: boolean;
  copiedStrokeWidth?: number;
}) {
  return (
    <AppMorphIcon
      icon={copied ? CheckData : CopyData}
      strokeWidth={copied ? copiedStrokeWidth : strokeWidth}
      {...props}
    />
  );
}

export type MorphIconElement = ElementRef<typeof BaseMorphIcon>;
