import type { RecentSessionDto } from "../../types";

export type SidebarSessionPlacement =
  | "all"
  | "pinned"
  | "project"
  | "automation"
  | "recent";

/**
 * Assign every ordinary sidebar session to exactly one visible section.
 *
 * Cron sessions may still own a project for workspace execution, but their
 * source is the stronger presentation signal. Pinning only changes placement;
 * it never changes project ownership.
 */
export function sidebarSessionPlacement(
  session: Pick<RecentSessionDto, "pinnedAt" | "source" | "projectId">,
): Exclude<SidebarSessionPlacement, "all"> {
  if (session.pinnedAt) return "pinned";
  if (session.source === "cron") return "automation";
  if (session.projectId) return "project";
  return "recent";
}

export function matchesSidebarSessionPlacement(
  session: Pick<RecentSessionDto, "pinnedAt" | "source" | "projectId">,
  placement: SidebarSessionPlacement,
): boolean {
  return placement === "all" || sidebarSessionPlacement(session) === placement;
}
