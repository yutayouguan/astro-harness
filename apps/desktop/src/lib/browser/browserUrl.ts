export function normalizeBrowserUrl(value: string): string {
  let trimmed = value.trim();
  if (!trimmed) return "";
  trimmed = trimmed.replace(
    /^(https?:\/\/)0\.0\.0\.0(?=[:/?#]|$)/i,
    (_match, scheme: string) => `${scheme}127.0.0.1`,
  );
  if (/^[a-z][a-z\d+.-]*:\/\//i.test(trimmed)) return trimmed;
  if (/^(localhost|127(?:\.\d{1,3}){3}|\[::1\])(?=[:/?#]|$)/i.test(trimmed)) {
    return `http://${trimmed}`;
  }
  return `https://${trimmed}`;
}
