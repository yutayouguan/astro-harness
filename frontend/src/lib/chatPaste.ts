/**
 * 聊天输入粘贴：从路径 / 剪贴板文件 / 图片像素构建附件。
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { ChatAttachment, ChatAttachmentKind } from "../types";
import { parseClipboardLocalPaths } from "./clipboardLocalPaths";

const MAX_INLINE_BYTES = 4 * 1024 * 1024;

type FileBase64Dto = {
  mime: string;
  size: number;
  base64: string;
  name: string;
};

function kindFromMime(mime: string, name: string): ChatAttachmentKind {
  if (mime.startsWith("image/")) return "image";
  if (mime.startsWith("video/")) return "video";
  if (mime.startsWith("audio/")) return "audio";
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  if (["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "heic"].includes(ext)) {
    return "image";
  }
  if (["mp4", "webm", "mov", "mkv", "avi"].includes(ext)) return "video";
  if (["mp3", "wav", "m4a", "aac", "ogg", "flac"].includes(ext)) return "audio";
  return "file";
}

function newAttId(): string {
  return `att-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

/** 将记忆沙箱内本地路径读成聊天附件 */
export async function pathToAttachment(path: string): Promise<ChatAttachment> {
  const dto = await invoke<FileBase64Dto>("read_file_base64", { path });
  const kind = kindFromMime(dto.mime, dto.name);
  const shouldInline =
    (kind === "image" && dto.size <= MAX_INLINE_BYTES) ||
    (kind === "file" &&
      dto.size <= 256 * 1024 &&
      (dto.mime.startsWith("text/") ||
        /\.(txt|md|json|csv|xml|yaml|yml|toml|rs|ts|tsx|js|py|html|css)$/i.test(
          dto.name,
        )));
  let previewUrl: string | undefined;
  if (kind === "image" || kind === "video") {
    try {
      previewUrl = convertFileSrc(path);
    } catch {
      previewUrl = undefined;
    }
  }
  return {
    id: newAttId(),
    name: dto.name,
    mime: dto.mime,
    kind,
    size: dto.size,
    previewUrl,
    dataBase64: shouldInline ? dto.base64 : undefined,
  };
}

/** 从系统文件剪贴板读取路径并转附件 */
export async function attachmentsFromOsClipboard(): Promise<ChatAttachment[]> {
  try {
    const paths = await invoke<string[]>("list_clipboard_file_paths");
    const out: ChatAttachment[] = [];
    for (const p of paths) {
      try {
        out.push(await pathToAttachment(p));
      } catch {
        // skip unreadable
      }
    }
    return out;
  } catch {
    return [];
  }
}

/** 从 navigator.clipboard.read() 提取图片 File */
export async function filesFromClipboardRead(): Promise<File[]> {
  if (!navigator.clipboard?.read) return [];
  try {
    const items = await navigator.clipboard.read();
    const files: File[] = [];
    let index = 0;
    for (const item of items) {
      for (const type of item.types) {
        if (!type.startsWith("image/")) continue;
        const blob = await item.getType(type);
        const ext = type.split("/")[1] || "png";
        files.push(
          new File([blob], `pasted-image-${index}.${ext}`, { type }),
        );
        index += 1;
      }
    }
    return files;
  } catch {
    return [];
  }
}

/** 纯文本是否应整段当作本地路径附件处理 */
export function pathsFromClipboardText(text: string): string[] {
  return parseClipboardLocalPaths(text);
}
