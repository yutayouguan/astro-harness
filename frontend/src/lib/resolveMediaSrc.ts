/**
 * 将聊天 / A2UI / 文件入口中的媒体 src 转为 WebView 可加载 URL。
 * 本地绝对路径与 file:// 走 Tauri convertFileSrc；其余可加载协议原样返回。
 */
import { convertFileSrc } from "@tauri-apps/api/core";

const PASSTHROUGH =
  /^(https?:|data:|blob:|asset:|tauri:|ipc:)/i;

/** 去掉 file:// 前缀（含多余斜杠），得到本地路径 */
export function stripFileUrl(src: string): string {
  const trimmed = src.trim();
  if (!/^file:/i.test(trimmed)) return trimmed;
  try {
    const u = new URL(trimmed);
    let path = decodeURIComponent(u.pathname);
    // Windows: /C:/Users/... → C:/Users/...
    if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
    return path;
  } catch {
    return trimmed.replace(/^file:\/\//i, "");
  }
}

/** 是否像本地绝对路径（POSIX / Windows） */
export function looksLikeLocalPath(src: string): boolean {
  const s = src.trim();
  if (!s) return false;
  if (PASSTHROUGH.test(s)) return false;
  if (/^file:/i.test(s)) return true;
  if (s.startsWith("/")) return true;
  if (/^[A-Za-z]:[\\/]/.test(s)) return true;
  if (s.startsWith("\\\\")) return true;
  return false;
}

/**
 * 解析为可加载 src；本地路径失败时返回 null（调用方显示 Broken）。
 * 非路径、非已知协议时原样返回（交给浏览器尝试）。
 */
export function resolveMediaSrc(src: string | null | undefined): string | null {
  if (src == null) return null;
  const raw = src.trim();
  if (!raw) return null;
  if (PASSTHROUGH.test(raw)) return raw;

  if (looksLikeLocalPath(raw)) {
    const path = stripFileUrl(raw);
    try {
      return convertFileSrc(path);
    } catch {
      return null;
    }
  }

  return raw;
}
