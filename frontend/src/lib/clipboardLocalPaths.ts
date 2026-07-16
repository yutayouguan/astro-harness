/**
 * 解析剪贴板纯文本是否为本地文件路径列表（用于聊天粘贴挂附件）。
 */
import {
  absolutizeMediaPath,
  looksLikeLocalPath,
  stripFileUrl,
} from "./resolveMediaSrc";

/**
 * 若文本整段都是本地路径（可多行），返回规范化路径列表；否则空数组。
 */
export function parseClipboardLocalPaths(text: string): string[] {
  const raw = text.replace(/\u0000/g, "").trim();
  if (!raw) return [];
  const lines = raw
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
  if (!lines.length) return [];
  const paths: string[] = [];
  for (const line of lines) {
    const stripped = stripFileUrl(line);
    const abs = absolutizeMediaPath(stripped);
    const candidate = abs ?? (looksLikeLocalPath(stripped) ? stripped : null);
    if (!candidate) return [];
    paths.push(candidate);
  }
  return paths;
}
