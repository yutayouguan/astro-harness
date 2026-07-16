/**
 * 跨平台路径展示：主目录缩为 `~`，分隔符统一为 `/`。
 * 仅用于 Toast / UI 文案，不参与真实文件 IO。
 */
export function displayUserPath(path: string): string {
  const raw = path.trim();
  if (!raw) return raw;
  let s = raw.replace(/\\/g, "/");

  if (/^file:\/\//i.test(s)) {
    try {
      const u = new URL(s);
      s = decodeURIComponent(u.pathname);
      if (/^\/[A-Za-z]:\//.test(s)) s = s.slice(1);
    } catch {
      s = s.replace(/^file:\/\//i, "");
    }
  }

  if (s.startsWith("~/") || s === "~") return s;

  const patterns = [
    /^\/Users\/[^/]+(?=\/|$)/, // macOS
    /^\/home\/[^/]+(?=\/|$)/, // Linux
    /^[A-Za-z]:\/Users\/[^/]+(?=\/|$)/, // Windows
  ];
  for (const re of patterns) {
    if (re.test(s)) {
      return s.replace(re, "~");
    }
  }
  return s;
}
