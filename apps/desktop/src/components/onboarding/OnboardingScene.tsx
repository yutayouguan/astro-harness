import { motion, useIsPresent, type HTMLMotionProps } from "framer-motion";
import { useEffect, useState, type CSSProperties } from "react";
import { ONBOARDING_WARP_MS } from "../../lib/ui/onboardingMotion";

/** Both page planes share one stage; only the arriving page can receive input. */
export function OnboardingScene({
  departing = false,
  reduced = false,
  portal = true,
  ...props
}: HTMLMotionProps<"section"> & {
  departing?: boolean;
  reduced?: boolean;
  portal?: boolean;
}) {
  const present = useIsPresent();
  const exiting = departing || !present;
  const [open, setOpen] = useState(!portal || reduced);
  useEffect(() => {
    if (reduced || !portal) {
      setOpen(true);
      return;
    }
    const timer = window.setTimeout(
      () => setOpen(true),
      ONBOARDING_WARP_MS.step + 120,
    );
    return () => window.clearTimeout(timer);
  }, [reduced, portal]);
  const revealing = portal && !reduced && !open && !exiting;
  return (
    <div
      className="onboarding-scene-window"
      data-exiting={exiting}
      data-portal={revealing}
      data-reduced={reduced}
      aria-hidden={exiting || undefined}
      {...(exiting ? { inert: "" } : {})}
      style={
        { "--portal-duration": `${ONBOARDING_WARP_MS.step}ms` } as CSSProperties
      }
    >
      <div
        className="onboarding-scene-aperture"
        onAnimationEnd={(event) => {
          if (
            event.target === event.currentTarget &&
            event.animationName === "onboarding-portal-open"
          )
            setOpen(true);
        }}
      >
        <motion.section {...props} />
      </div>
      {revealing && <div className="onboarding-portal-rim" aria-hidden />}
    </div>
  );
}
