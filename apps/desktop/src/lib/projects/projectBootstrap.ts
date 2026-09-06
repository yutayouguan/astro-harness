import type { ProjectDto } from "../../types";

export const DEFAULT_PROJECT_PLACEHOLDER: ProjectDto = {
  id: "default",
  name: "主空间",
  icon: "astro-space",
  roots: [],
  position: 0,
  createdAt: "",
  updatedAt: "",
};

const PROJECT_LOAD_RETRY_DELAYS_MS = [0, 250, 1_000] as const;

export function ensureDefaultProjectVisible(
  projects: ProjectDto[] | null | undefined,
): ProjectDto[] {
  const list = projects ?? [];
  return list.some((project) => project.id === DEFAULT_PROJECT_PLACEHOLDER.id)
    ? list
    : [DEFAULT_PROJECT_PLACEHOLDER, ...list];
}

export async function loadProjectsWithRetry(
  load: () => Promise<ProjectDto[] | null | undefined>,
  wait: (delayMs: number) => Promise<void> = (delayMs) =>
    new Promise((resolve) => window.setTimeout(resolve, delayMs)),
): Promise<ProjectDto[]> {
  let lastError: unknown = new Error("project list is empty");
  for (const delayMs of PROJECT_LOAD_RETRY_DELAYS_MS) {
    if (delayMs > 0) await wait(delayMs);
    try {
      const projects = await load();
      if (!projects?.length) {
        lastError = new Error("project list is empty");
        continue;
      }
      return ensureDefaultProjectVisible(projects);
    } catch (error) {
      lastError = error;
    }
  }
  throw lastError;
}
