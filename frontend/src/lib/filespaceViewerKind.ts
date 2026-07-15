/** 文件空间右侧 Viewer 类型判定（纯函数，便于单测） */
import { fileExt, isPdfFile, mediaKindOf, resolveFileType } from "./fileTypeIcon.ts";

export type FilespaceViewerKind =
  | "markdown"
  | "html"
  | "text"
  | "image"
  | "video"
  | "audio"
  | "pdf"
  | "external"
  | "missing";

const MD_EXTS = new Set(["md", "markdown", "mdx"]);

export type FilespaceViewerInput = {
  name: string;
  missing?: boolean;
  mime?: string | null;
  category?: string | null;
};

/** artifact → 右侧 Viewer 分支 */
export function filespaceViewerKind(input: FilespaceViewerInput): FilespaceViewerKind {
  if (input.missing) return "missing";

  const media = mediaKindOf(input.name);
  if (media === "image") return "image";
  if (media === "video") return "video";
  if (media === "audio") return "audio";
  if (media === "html") return "html";
  if (isPdfFile(input.name)) return "pdf";

  const ext = fileExt(input.name);
  if (MD_EXTS.has(ext)) return "markdown";

  const open = resolveFileType(input.name, false).open;
  if (open === "text") return "text";

  const mime = input.mime ?? "";
  if (
    mime.startsWith("text/") ||
    mime === "application/json" ||
    mime === "application/xml"
  ) {
    return "text";
  }

  const cat = input.category;
  if (cat === "code" || cat === "doc" || cat === "sheet") return "text";

  return "external";
}
