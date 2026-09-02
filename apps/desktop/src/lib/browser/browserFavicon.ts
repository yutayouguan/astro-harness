const MAX_BROWSER_FAVICON_URL_LENGTH = 8_192;
const SAFE_DATA_IMAGE =
  /^data:image\/(?:png|jpeg|gif|webp|x-icon|vnd\.microsoft\.icon);base64,/i;

export function normalizeBrowserFaviconUrl(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const candidate = value.trim();
  if (!candidate || candidate.length > MAX_BROWSER_FAVICON_URL_LENGTH) {
    return null;
  }
  if (SAFE_DATA_IMAGE.test(candidate)) return candidate;
  try {
    const url = new URL(candidate);
    return url.protocol === "http:" || url.protocol === "https:"
      ? url.href
      : null;
  } catch {
    return null;
  }
}
