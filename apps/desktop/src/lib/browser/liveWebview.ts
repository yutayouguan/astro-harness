export const LIVE_BROWSER_WEBVIEW_PREFIX = "astro-browser-live-";

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
