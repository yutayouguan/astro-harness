/**
 * 媒体预览：下载到系统「下载」目录、复制到剪贴板（文件引用 / 图片像素 / 路径）。
 */
import { invoke } from "@tauri-apps/api/core";
import { parseClipboardLocalPaths } from "./clipboardLocalPaths";
import {
  absolutizeMediaPath,
  looksLikeLocalPath,
  stripFileUrl,
} from "./resolveMediaSrc";

export { parseClipboardLocalPaths };

export type MediaActionKind =
  "image" | "video" | "audio" | "html" | "code" | "document";

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

async function copyImagePixels(local: string): Promise<void> {
  const dto = await invoke<FileBase64Dto>("read_file_base64", {
    path: local,
  });
  const bytes = base64ToBytes(dto.base64);
  const mime = dto.mime.startsWith("image/") ? dto.mime : "image/png";
  const imageBlob = new Blob([bytes.buffer as ArrayBuffer], { type: mime });
  // 附带绝对路径，便于聊天输入框把粘贴识别为附件而非纯文本
  const textBlob = new Blob([local], { type: "text/plain" });
  await navigator.clipboard.write([
    new ClipboardItem({
      [mime]: imageBlob,
      "text/plain": textBlob,
    }),
  ]);
}

/**
 * 复制媒体：
 * - 本地文件：优先系统文件剪贴板（聊天输入可粘贴为附件）
 * - 图片：文件剪贴板失败时再写像素 + 路径文本
 * - 最后退化为路径文本
 */
export async function copyMedia(
  path: string,
  kind: MediaActionKind,
): Promise<CopyMediaResult> {
  const local = mediaLocalPath(path);

  // 代码/文本文件：优先复制文本内容（而非文件引用）
  if (kind === "code" && local) {
    try {
      const text = await invoke<string>("read_file", { path: local });
      await navigator.clipboard.writeText(text);
      return "text";
    } catch {
      // fall through to file/path 复制
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

  if (kind === "image" && local && typeof ClipboardItem !== "undefined") {
    try {
      await copyImagePixels(local);
      return "image";
    } catch {
      // fall through — 部分 WebView 不接受 image+text 组合
      try {
        const dto = await invoke<FileBase64Dto>("read_file_base64", {
          path: local,
        });
        const bytes = base64ToBytes(dto.base64);
        const mime = dto.mime.startsWith("image/") ? dto.mime : "image/png";
        const blob = new Blob([bytes.buffer as ArrayBuffer], { type: mime });
        await navigator.clipboard.write([new ClipboardItem({ [mime]: blob })]);
        return "image";
      } catch {
        // fall through
      }
    }
  }

  // 远程图片：尽量写入像素，便于粘贴
  if (
    kind === "image" &&
    /^https?:/i.test(path.trim()) &&
    typeof ClipboardItem !== "undefined"
  ) {
    try {
      const res = await fetch(path.trim());
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const buf = new Uint8Array(await res.arrayBuffer());
      const mime =
        res.headers.get("content-type")?.split(";")[0]?.trim() || "image/png";
      const type = mime.startsWith("image/") ? mime : "image/png";
      const blob = new Blob([buf.buffer as ArrayBuffer], { type });
      await navigator.clipboard.write([new ClipboardItem({ [type]: blob })]);
      return "image";
    } catch {
      // fall through
    }
  }

  await navigator.clipboard.writeText(local ?? path.trim());
  return "text";
}
