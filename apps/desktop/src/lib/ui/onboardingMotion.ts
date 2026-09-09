/** One timing contract for the portal, stage changes, and App handoff. */
export const ONBOARDING_WARP_MS = {
  intro: 2200,
  step: 760,
  app: 980,
  reduced: 160,
} as const;

const depth = (z: number) => `perspective(1200px) translateZ(${z}px)`;

export function onboardingStageVariants(reduced: boolean) {
  if (reduced) {
    return {
      enter: { opacity: 0 },
      center: { opacity: 1, transition: { duration: 0.12 } },
      exit: { opacity: 0, transition: { duration: 0.08 } },
    };
  }
  return {
    enter: (direction: number) => ({
      opacity: 0,
      transform: depth(direction >= 0 ? -1100 : 650),
    }),
    center: {
      opacity: 1,
      transform: depth(0),
      transition: {
        transform: { duration: 0.68, ease: [0.32, 0.05, 0.25, 1] as const },
        opacity: { duration: 0.3, delay: 0.08, ease: "linear" as const },
      },
    },
    // AnimatePresence passes the latest direction, including when reversing a step.
    exit: (direction: number) => ({
      opacity: 0,
      transform: depth(direction >= 0 ? 650 : -1100),
      transition: {
        transform: { duration: 0.44, ease: [0.4, 0, 0.65, 1] as const },
        opacity: { duration: 0.26, delay: 0.1, ease: "linear" as const },
      },
    }),
  };
}
