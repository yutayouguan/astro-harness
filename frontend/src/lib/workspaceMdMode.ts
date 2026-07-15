/** 工作空间 Markdown 预览/源码模式持久化与判定。 */

export type MdMode = "preview" | "source";

export const WORKSPACE_MD_MODE_KEY = "astro.workspace.mdPreviewMode";

/** 是否为可切换预览的 Markdown 文件名 */
export function isMarkdownFilename(filename: string): boolean {
  const lower = filename.toLowerCase();
  return lower.endsWith(".md") || lower.endsWith(".markdown");
}

/** 读取上次模式；无效或不可读时默认 source */
export function readWorkspaceMdMode(): MdMode {
  try {
    const v = localStorage.getItem(WORKSPACE_MD_MODE_KEY);
    if (v === "source" || v === "preview") return v;
  } catch {
    // ignore
  }
  return "source";
}

/** 写入模式；失败静默 */
export function writeWorkspaceMdMode(mode: MdMode): void {
  try {
    localStorage.setItem(WORKSPACE_MD_MODE_KEY, mode);
  } catch {
    // ignore
  }
}
