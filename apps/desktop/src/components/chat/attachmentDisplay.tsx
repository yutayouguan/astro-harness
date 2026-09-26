/** 附件展示共用：容量文案与类型图标（消息行与输入区都要用）。 */
import { File, FileVideo, FolderOpen, Image, Music2 } from "lucide-react";
import type { ChatAttachmentKind } from "../../types";

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function AttachmentGlyph({ kind }: { kind: ChatAttachmentKind }) {
  if (kind === "folder") {
    return <FolderOpen size={16} strokeWidth={2} aria-hidden />;
  }
  if (kind === "image") {
    return <Image size={16} strokeWidth={2} aria-hidden />;
  }
  if (kind === "video") {
    return <FileVideo size={16} strokeWidth={2} aria-hidden />;
  }
  if (kind === "audio") {
    return <Music2 size={16} strokeWidth={2} aria-hidden />;
  }
  return <File size={16} strokeWidth={2} aria-hidden />;
}
