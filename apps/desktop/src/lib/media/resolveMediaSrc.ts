/**
 * 将聊天 / A2UI / 文件入口中的媒体 src 转为 WebView 可加载 URL。
 * 本地绝对路径与 file:// 走 Tauri convertFileSrc；
 * 工作区相对路径（如 generated/xxx.png）在提供 baseDir 时先拼成绝对路径再转换。
 */
import { convertFileSrc } from "@tauri-apps/api/core";

const PASSTHROUGH =
  /^(https?:|data:|blob:|asset:|tauri:|ipc:)/i;

/**
 * Markdown / 浏览器常把中文文件名编成 `%E4%BA%91…`。
 * 按路径段安全解码，避免把真实文件系统名对不上。
 */
export function decodeMediaPathEncoding(path: string): string {
  if (!/%[0-9A-Fa-f]{2}/.test(path)) return path;
  return path
    .split(/([/\\])/)
    .map((part) => {
      if (part === "/" || part === "\\") return part;
      try {
        return decodeURIComponent(part);
      } catch {
        return part;
      }
    })
    .join("");
}

/** 去掉 file:// 前缀（含多余斜杠），得到本地路径 */
export function stripFileUrl(src: string): string {
  const trimmed = src.trim();
  if (!/^file:/i.test(trimmed)) return decodeMediaPathEncoding(trimmed);
  try {
    const u = new URL(trimmed);
    let path = decodeURIComponent(u.pathname);
    // Windows: /C:/Users/... → C:/Users/...
    if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
    return decodeMediaPathEncoding(path);
  } catch {
    return decodeMediaPathEncoding(trimmed.replace(/^file:\/\//i, ""));
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

/** 是否像工作区内相对本地路径（非协议、非绝对） */
export function looksLikeRelativeLocalPath(src: string): boolean {
  const s = src.trim();
  if (!s) return false;
  if (PASSTHROUGH.test(s)) return false;
  if (looksLikeLocalPath(s)) return false;
  // 排除纯网盘式短协议误判；允许 generated/foo.png、./a.webp
  return true;
}

/**
 * 将媒体 src 规范为本地绝对路径。
 * http(s)/data 等返回 null；相对路径无 baseDir 或含 `..` 时返回 null。
 */
export function absolutizeMediaPath(
  src: string | null | undefined,
  baseDir?: string | null,
): string | null {
  if (src == null) return null;
  const raw = decodeMediaPathEncoding(src.trim());
  if (!raw) return null;
  if (PASSTHROUGH.test(raw)) return null;

  if (looksLikeLocalPath(raw)) {
    return stripFileUrl(raw);
  }

  if (!looksLikeRelativeLocalPath(raw)) return null;
  const base = baseDir?.trim();
  if (!base) return null;

  const rel = raw.replace(/^\.\//, "").replace(/^[/\\]+/, "");
  const segments = rel.split(/[/\\]/).filter((p) => p && p !== ".");
  if (segments.some((p) => p === "..")) return null;
  if (segments.length === 0) return null;

  const baseClean = base.replace(/[/\\]+$/, "");
  const sep = /\\/.test(baseClean) && !/\//.test(baseClean) ? "\\" : "/";
  return `${baseClean}${sep}${segments.join(sep)}`;
}

/** 生成媒体预览使用绝对本地路径，远程 URL 等则保持原值。 */
export function resolveMediaPreviewPath(
  src: string,
  baseDir?: string | null,
): string {
  return absolutizeMediaPath(src, baseDir) ?? src.trim();
}

/**
 * 解析为可加载 src；本地路径失败时返回 null（调用方显示 Broken）。
 * 非路径、非已知协议时原样返回（交给浏览器尝试）。
 * @param baseDir Agent 工作区根目录，用于解析 Markdown 中的相对路径
 */
export function resolveMediaSrc(
  src: string | null | undefined,
  baseDir?: string | null,
): string | null {
  if (src == null) return null;
  const raw = src.trim();
  if (!raw) return null;
  if (PASSTHROUGH.test(raw)) return raw;

  const abs = absolutizeMediaPath(raw, baseDir);
  if (abs) {
    try {
      return convertFileSrc(abs);
    } catch {
      return null;
    }
  }

  // 相对路径缺 baseDir：不当作可加载 URL，避免 WebView 相对解析失败成 Broken 前白闪
  if (looksLikeRelativeLocalPath(raw)) return null;

  return raw;
}
