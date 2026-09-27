/**
 * 网页链接的默认去向：交给 Astro 内置浏览器（侧边浏览器坞）。
 *
 * 只在浏览器坞可用时接管（内置浏览器是聊天页的右侧坞），并且只接管普通左键点击：
 * 修饰键点击仍然走 WebView 默认行为，需要时可以用系统浏览器打开。
 */

/** 显式要求走系统浏览器的标记：`<a data-browser-target="external">`。 */
export const IN_APP_BROWSER_OPT_OUT_ATTR = "data-browser-target";
export const IN_APP_BROWSER_OPT_OUT_VALUE = "external";

/** 只接管网页链接；`mailto:` / `file:` / 相对路径交给各自的既有处理。 */
export function isWebLinkHref(href: string | null | undefined): boolean {
  const raw = href?.trim();
  if (!raw) return false;
  try {
    const protocol = new URL(raw).protocol;
    return protocol === "http:" || protocol === "https:";
  } catch {
    return false;
  }
}

/** 是否运行在 Tauri 壳内；浏览器预览 / Storybook 里没有内置浏览器可用。 */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** 与点击事件相关的字段子集，便于脱离 DOM 单测。 */
export type WebLinkClick = {
  defaultPrevented?: boolean;
  button?: number;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  altKey?: boolean;
  target?: unknown;
};

function findAnchor(target: unknown): Element | null {
  const closest = (target as { closest?: unknown } | null | undefined)?.closest;
  if (typeof closest !== "function") return null;
  return (target as Element).closest("a[href]");
}

/**
 * 判断这次点击是否应该在内置浏览器打开，返回要打开的 URL。
 *
 * 返回 null 表示不接管：调用方保持 WebView 默认行为（新窗口 / 系统浏览器）。
 */
export function resolveInAppBrowserLink(
  event: WebLinkClick,
  options: { dockAvailable: boolean },
): string | null {
  if (!options.dockAvailable) return null;
  if (event.defaultPrevented) return null;
  // 只处理普通左键；Cmd/Ctrl/Shift 点击交给浏览器默认行为。
  if ((event.button ?? 0) !== 0) return null;
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return null;
  }

  const anchor = findAnchor(event.target);
  if (!anchor) return null;
  if (
    anchor.getAttribute(IN_APP_BROWSER_OPT_OUT_ATTR) ===
    IN_APP_BROWSER_OPT_OUT_VALUE
  ) {
    return null;
  }

  const href = anchor.getAttribute("href");
  return isWebLinkHref(href) ? href!.trim() : null;
}
