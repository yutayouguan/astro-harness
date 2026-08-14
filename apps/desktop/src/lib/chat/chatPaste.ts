/**
 * 聊天输入粘贴 / 拖放：从路径或剪贴板构建附件。
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { ChatAttachment, ChatAttachmentKind } from "../../types";
import { parseClipboardLocalPaths } from "../media/clipboardLocalPaths";

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

function dtoToAttachment(path: string, dto: FileBase64Dto): ChatAttachment {
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
    localPath: path,
  };
}

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
}

async function remoteUrlToAttachment(url: string): Promise<ChatAttachment> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const buf = new Uint8Array(await res.arrayBuffer());
  if (buf.byteLength > 32 * 1024 * 1024) {
    throw new Error("文件过大（>32MB）");
  }
  const mime =
    res.headers.get("content-type")?.split(";")[0]?.trim() || "image/png";
  let name = "image.png";
  try {
    const last = new URL(url).pathname.split("/").filter(Boolean).pop();
    if (last) name = decodeURIComponent(last);
  } catch {
    // keep default
  }
  const kind = kindFromMime(mime, name);
  const previewUrl = kind === "image" || kind === "video" ? url : undefined;
  const shouldInline = kind === "image" && buf.byteLength <= MAX_INLINE_BYTES;
  return {
    id: newAttId(),
    name,
    mime: mime.startsWith("image/") ? mime : "image/png",
    kind: kind === "image" ? "image" : kind,
    size: buf.byteLength,
    previewUrl,
    dataBase64: shouldInline ? bytesToBase64(buf) : undefined,
  };
}

/** 将本地绝对路径或 http(s) 媒体读成聊天附件 */
export async function pathToAttachment(path: string): Promise<ChatAttachment> {
  const raw = path.trim();
  if (/^https?:/i.test(raw)) {
    return remoteUrlToAttachment(raw);
  }
  try {
    const dto = await invoke<FileBase64Dto>("read_file_base64", { path: raw });
    return dtoToAttachment(raw, dto);
  } catch {
    const dto = await invoke<FileBase64Dto>("read_user_file_base64", {
      path: raw,
    });
    return dtoToAttachment(raw, dto);
  }
}

/** 批量路径 → 附件（跳过失败项） */
export async function pathsToAttachments(
  paths: string[],
): Promise<ChatAttachment[]> {
  const out: ChatAttachment[] = [];
  for (const p of paths) {
    const trimmed = p.trim();
    if (!trimmed) continue;
    try {
      out.push(await pathToAttachment(trimmed));
    } catch {
      // skip unreadable / directories
    }
  }
  return out;
}

/** 从系统文件剪贴板读取路径并转附件 */
export async function attachmentsFromOsClipboard(): Promise<ChatAttachment[]> {
  try {
    const paths = await invoke<string[]>("list_clipboard_file_paths");
    return pathsToAttachments(paths);
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

/** 从 HTML5 DataTransfer 解析 file:// / 本地路径列表 */
export function pathsFromDataTransfer(dt: DataTransfer | null): string[] {
  if (!dt) return [];
  const uriList = dt.getData("text/uri-list");
  if (uriList.trim()) {
    return parseClipboardLocalPaths(
      uriList
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter((l) => l && !l.startsWith("#"))
        .join("\n"),
    );
  }
  const text = dt.getData("text/plain");
  return pathsFromClipboardText(text);
}
