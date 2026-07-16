/**
 * 媒体预览：下载到系统「下载」目录、复制到剪贴板（图片像素 / 文件 / 路径）。
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

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    bin += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(bin);
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

/**
 * 下载到系统「下载」目录，返回保存后的绝对路径。
 * 本地文件经 Tauri 复制；远程 http(s) 先 fetch 再写入下载目录。
 */
export async function downloadMedia(path: string): Promise<string> {
  const local = mediaLocalPath(path);
  if (local) {
    return invoke<string>("download_file_to_downloads", { path: local });
  }
  if (/^https?:/i.test(path.trim())) {
    const res = await fetch(path.trim());
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const buf = new Uint8Array(await res.arrayBuffer());
    if (buf.byteLength > 32 * 1024 * 1024) {
      throw new Error("文件过大（>32MB）");
    }
    return invoke<string>("download_bytes_to_downloads", {
      filename: filenameFromUrl(path.trim()),
      base64Data: bytesToBase64(buf),
    });
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
