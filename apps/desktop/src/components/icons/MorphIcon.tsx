import { forwardRef, type ComponentProps, type ElementRef } from "react";
import {
  MorphIcon as BaseMorphIcon,
  type IconInput,
  type MorphHandle,
} from "morphicons/react";
import type { IconNode as LucideIconNode } from "lucide";
import { Check as CheckData, Copy as CopyData } from "lucide";
import { useMorphicons } from "../../hooks/app/useMorphicons";

export type MorphableIcon = IconInput | LucideIconNode;

function normalizeIconData(icon: MorphableIcon): IconInput {
  if (Array.isArray(icon) && icon[0] === "svg" && Array.isArray(icon[2])) {
    return icon[2] as IconInput;
  }
  return icon as IconInput;
}

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
        icon={normalizeIconData(icon)}
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
  ...props
}: Omit<AppMorphIconProps, "icon"> & { copied: boolean }) {
  return (
    <AppMorphIcon
      icon={copied ? CheckData : CopyData}
      {...props}
    />
  );
}

export type MorphIconElement = ElementRef<typeof BaseMorphIcon>;
