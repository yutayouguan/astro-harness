export const LIVE_BROWSER_WEBVIEW_PREFIX = "astro-browser-live-";

export type BrowserLiveWebviewHandle = {
  hide: () => Promise<void>;
  close: () => Promise<void>;
};

/**
 * WKWebView omits the `Safari/...` product token even though it runs the same
 * system WebKit engine. Some sites (including bilibili.com) treat that
 * otherwise-valid UA as an obsolete browser.
 *
 * Only patch the token-less macOS WebKit shape. Chromium/WebView2 and regular
 * Safari keep their native UA so sites never receive a mismatched engine name.
 */
export function liveBrowserUserAgentOverride(
  userAgent: string,
): string | undefined {
  const normalized = userAgent.trim();
  const webKitVersion = normalized.match(/\bAppleWebKit\/([\d.]+)/)?.[1];
  if (
    !normalized.includes("Macintosh") ||
    !webKitVersion ||
    /\bSafari\/[\d.]+/.test(normalized)
  ) {
    return undefined;
  }
  return `${normalized} Safari/${webKitVersion}`;
}

/**
 * Native child WebViews render above the DOM. Hide first so a failed or slow
 * close cannot leave an interactive surface covering the rest of the app.
 */
export async function retireBrowserLiveWebview(
  webview: BrowserLiveWebviewHandle,
): Promise<void> {
  try {
    await webview.hide();
  } catch {
    // The WebView may still be finishing creation; closing remains worthwhile.
  }
  try {
    await webview.close();
  } catch {
    // Cleanup is best-effort and is retried by the next orphan sweep.
  }
}

let surfaceSequence = 0;

function hashLabelPart(value: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(36);
}

export function createBrowserLiveSurfaceId(now = Date.now()): string {
  surfaceSequence += 1;
  return `${now.toString(36)}-${surfaceSequence.toString(36)}`;
}

export function browserLiveWebviewLabel(
  sessionId: string,
  tabId: string,
  surfaceId: string,
): string {
  return `${LIVE_BROWSER_WEBVIEW_PREFIX}${hashLabelPart(surfaceId)}-${hashLabelPart(sessionId)}-${hashLabelPart(tabId)}`;
}

export function canonicalBrowserUrl(raw: string): string {
  try {
    return new URL(raw).href;
  } catch {
    return raw.trim();
  }
}

export function browserWebviewBoundsKey(bounds: {
  x: number;
  y: number;
  width: number;
  height: number;
}): string {
  return `${bounds.x}:${bounds.y}:${bounds.width}:${bounds.height}`;
}

export function resolveLiveDesiredUrl(
  previewUrl: string,
  pendingNativeUrl?: string,
): string {
  if (
    pendingNativeUrl &&
    canonicalBrowserUrl(pendingNativeUrl) !== canonicalBrowserUrl(previewUrl)
  ) {
    return pendingNativeUrl;
  }
  return previewUrl;
}
