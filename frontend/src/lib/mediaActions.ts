/**
 * 媒体预览：下载到本机、复制到剪贴板（图片像素 / 文件 / 路径）。
 */
import { invoke } from "@tauri-apps/api/core";
import {
  absolutizeMediaPath,
  looksLikeLocalPath,
  stripFileUrl,
} from "./resolveMediaSrc";

export type MediaActionKind = "image" | "video" | "audio" | "html" | "document";

type FileBase64Dto = {
  mime: string;
  size: number;
  base64: string;
  name: string;
};

/** 解析为可供 Tauri 读写的本地绝对路径；远程 URL 返回 null */
export function mediaLocalPath(path: string | null | undefined): string | null {
  if (path == null) return null;
  const raw = path.trim();
  if (!raw) return null;
  if (/^(https?:|data:|blob:)/i.test(raw)) return null;
  const abs = absolutizeMediaPath(raw);
  if (abs) return abs;
  if (looksLikeLocalPath(raw)) return stripFileUrl(raw);
  return null;
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

function triggerBlobDownload(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename || "download";
  a.rel = "noopener";
  document.body.appendChild(a);
  a.click();
  a.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1500);
}

function filenameFromUrl(url: string): string {
  try {
    const u = new URL(url);
    const last = u.pathname.split("/").filter(Boolean).pop();
    if (last) return decodeURIComponent(last);
  } catch {
    // ignore
  }
  return "download";
}

/** 触发浏览器「另存为」式下载（本地经 read_file_base64；远程 fetch） */
export async function downloadMedia(path: string): Promise<void> {
  const local = mediaLocalPath(path);
  if (local) {
    const dto = await invoke<FileBase64Dto>("read_file_base64", { path: local });
    const bytes = base64ToBytes(dto.base64);
    const blob = new Blob([bytes.buffer as ArrayBuffer], {
      type: dto.mime || "application/octet-stream",
    });
    triggerBlobDownload(blob, dto.name);
    return;
  }
  if (/^https?:/i.test(path.trim())) {
    const res = await fetch(path.trim());
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const blob = await res.blob();
    triggerBlobDownload(blob, filenameFromUrl(path.trim()));
    return;
  }
  throw new Error("无法下载此媒体");
}

export type CopyMediaResult = "image" | "file" | "text";

/**
 * 复制媒体：
 * - 图片：优先剪贴板图片；失败则复制文件 / 路径
 * - 其它：优先系统剪贴板文件；再退化为路径文本
 */
export async function copyMedia(
  path: string,
  kind: MediaActionKind,
): Promise<CopyMediaResult> {
  const local = mediaLocalPath(path);

  if (kind === "image" && local && typeof ClipboardItem !== "undefined") {
    try {
      const dto = await invoke<FileBase64Dto>("read_file_base64", {
        path: local,
      });
      const bytes = base64ToBytes(dto.base64);
      const mime = dto.mime.startsWith("image/") ? dto.mime : "image/png";
      const blob = new Blob([bytes.buffer as ArrayBuffer], { type: mime });
      await navigator.clipboard.write([
        new ClipboardItem({ [blob.type]: blob }),
      ]);
      return "image";
    } catch {
      // fall through
    }
  }

  if (local) {
    try {
      await invoke("copy_paths_to_clipboard", { paths: [local] });
      return "file";
    } catch {
      // fall through
    }
  }

  await navigator.clipboard.writeText(path.trim());
  return "text";
}
