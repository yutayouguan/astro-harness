/** One timing contract for the portal, stage changes, and App handoff. */
export const ONBOARDING_WARP_MS = {
  intro: 2800,
  step: 1100,
  app: 1400,
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
      transform: depth(direction >= 0 ? -650 : 500),
    }),
    center: {
      opacity: 1,
      transform: depth(0),
      transition: {
        transform: { duration: 1.02, ease: [0.4, 0, 0.2, 1] as const },
        opacity: { duration: 0.45, delay: 0.16, ease: "linear" as const },
      },
    },
    // AnimatePresence passes the latest direction, including when reversing a step.
    exit: (direction: number) => ({
      opacity: 0,
      transform: depth(direction >= 0 ? 500 : -650),
      transition: {
        transform: { duration: 0.7, ease: [0.4, 0, 0.4, 1] as const },
        opacity: { duration: 0.46, delay: 0.08, ease: "linear" as const },
      },
    }),
  };
}
