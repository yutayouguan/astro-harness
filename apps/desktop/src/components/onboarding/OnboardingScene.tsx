import { motion, useIsPresent, type HTMLMotionProps } from "framer-motion";

/** Both page planes share one stage; only the arriving page can receive input. */
export function OnboardingScene({
  departing = false,
  ...props
}: HTMLMotionProps<"section"> & { departing?: boolean }) {
  const present = useIsPresent();
  const exiting = departing || !present;
  return (
    <motion.section
      {...props}
      data-exiting={exiting}
      aria-hidden={exiting || undefined}
      {...(exiting ? { inert: "" } : {})}
    />
  );
}
