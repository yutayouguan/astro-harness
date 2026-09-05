export type ChatRightDock =
  | "project-files"
  | "browser"
  | "side-chat"
  | "inspector"
  | "review";

type ChatRightDockState = {
  projectFilesOpen: boolean;
  browserOpen: boolean;
  sideSessionOpen: boolean;
  inspectorOpen: boolean;
  reviewOpen: boolean;
};

/**
 * The right edge is a single spatial destination. Historic persisted state can
 * briefly expose more than one open flag, so resolve it to exactly one surface.
 */
export function resolveChatRightDock({
  projectFilesOpen,
  browserOpen,
  sideSessionOpen,
  inspectorOpen,
  reviewOpen,
}: ChatRightDockState): ChatRightDock | null {
  if (reviewOpen) return "review";
  if (browserOpen) return "browser";
  if (projectFilesOpen) return "project-files";
  if (sideSessionOpen) return "side-chat";
  if (inspectorOpen) return "inspector";
  return null;
}
