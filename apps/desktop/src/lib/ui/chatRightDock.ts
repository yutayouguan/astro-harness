export type ChatRightDock = "project-files" | "side-chat" | "inspector" | "review";

type ChatRightDockState = {
  projectFilesOpen: boolean;
  sideSessionOpen: boolean;
  inspectorOpen: boolean;
  reviewOpen: boolean;
};

type ChatRightDockWidths = {
  projectFiles: number;
  inspector: number;
  review: number;
  sideChat?: number;
};

export const CHAT_SIDE_PANEL_WIDTH = 384;

/**
 * The right edge is a single spatial destination. Historic persisted state can
 * briefly expose more than one open flag, so resolve it to exactly one surface.
 */
export function resolveChatRightDock({
  projectFilesOpen,
  sideSessionOpen,
  inspectorOpen,
  reviewOpen,
}: ChatRightDockState): ChatRightDock | null {
  if (reviewOpen) return "review";
  if (projectFilesOpen) return "project-files";
  if (sideSessionOpen) return "side-chat";
  if (inspectorOpen) return "inspector";
  return null;
}

/** Header chrome yields to the active dock only, never to a sum of hidden panels. */
export function chatRightDockWidth(
  dock: ChatRightDock | null,
  widths: ChatRightDockWidths,
): number {
  if (dock === "project-files") return widths.projectFiles;
  if (dock === "side-chat") return widths.sideChat ?? CHAT_SIDE_PANEL_WIDTH;
  if (dock === "inspector") return widths.inspector;
  if (dock === "review") return widths.review;
  return 0;
}
