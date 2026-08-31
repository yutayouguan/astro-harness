/**
 * 把内嵌 HTML 里的相对资源引用（<img src> / poster / CSS url(...)）改写成
 * 指向文件所在目录的绝对 asset URL。
 *
 * 为什么不用 <base href>：Tauri 的 convertFileSrc 会用 encodeURIComponent 把整个
 * 绝对路径编成单个 path 段（`/` → `%2F`）。当它作为 <base href> 时，HTML 里相对
 * 引用中的 `../` 会把这唯一的段整体弹掉、退回 asset 根，得到相对路径（如
 * `images/x.jpg`），落不进 assetProtocol.scope，于是被拒绝。逐个引用解析成绝对
 * 路径后再 convertFileSrc，产出的就是 app 内其它媒体同款、已被 scope 放行的格式。
 */

/** 需要跳过的（已是可加载 URL / 绝对路径 / 锚点 / 协议相对）引用 */
function isRewritableRelative(url: string): boolean {
  const s = url.trim();
  if (!s) return false;
  if (s.startsWith("//")) return false; // 协议相对
  if (/^[A-Za-z][A-Za-z0-9+.-]*:/.test(s)) return false; // 任意协议 http:/data:/asset:/blob:/mailto: …
  if (s.startsWith("#")) return false; // 页内锚点
  if (s.startsWith("/")) return false; // POSIX 绝对
  if (/^[A-Za-z]:[\\/]/.test(s)) return false; // Windows 绝对
  if (s.startsWith("\\\\")) return false; // UNC
  return true;
}

/** 目录是否为绝对本地路径（无绝对目录则无法拼出正确 asset URL） */
export function isAbsoluteDir(dir: string): boolean {
  return (
    dir.startsWith("/") || /^[A-Za-z]:[\\/]/.test(dir) || dir.startsWith("\\\\")
  );
}

/** 按目录解析相对引用为绝对本地路径；段做 percent 解码以还原真实文件名 */
export function resolveAgainstDir(dir: string, rel: string): string {
  const cleanRel = rel.replace(/[?#].*$/, "");
  const isWin = /\\/.test(dir) && !/\//.test(dir);
  const sep = isWin ? "\\" : "/";
  const base = dir.replace(/[\\/]+$/, "").split(/[\\/]/);
  for (const raw of cleanRel.split(/[\\/]/)) {
    if (raw === "" || raw === ".") continue;
    if (raw === "..") {
      if (base.length > 1) base.pop();
      continue;
    }
    let seg = raw;
    try {
      seg = decodeURIComponent(raw);
    } catch {
      seg = raw;
    }
    base.push(seg);
  }
  return base.join(sep);
}

/**
 * 改写 HTML 中的相对资源引用。
 * @param html 原始 HTML
 * @param dir 该 HTML 文件所在目录（须为绝对路径）
 * @param convert Tauri convertFileSrc（注入以便测试）
 */
export function rewriteHtmlRelativeAssets(
  html: string,
  dir: string,
  convert: (path: string) => string,
): string {
  if (!isAbsoluteDir(dir)) return html;

  const toUrl = (rel: string): string | null => {
    if (!isRewritableRelative(rel)) return null;
    try {
      return convert(resolveAgainstDir(dir, rel));
    } catch {
      return null;
    }
  };

  let out = html.replace(
    /\b(src|poster)\s*=\s*("|')([^"']*)\2/gi,
    (m, attr: string, q: string, val: string) => {
      const u = toUrl(val);
      return u ? `${attr}=${q}${u}${q}` : m;
    },
  );

  out = out.replace(
    /url\(\s*(['"]?)([^'")]+)\1\s*\)/gi,
    (m, q: string, val: string) => {
      const u = toUrl(val);
      return u ? `url(${q}${u}${q})` : m;
    },
  );

  return out;
}
