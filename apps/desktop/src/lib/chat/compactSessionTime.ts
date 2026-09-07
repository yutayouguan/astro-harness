const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

function pad2(value: number): string {
  return String(value).padStart(2, "0");
}

/** 侧栏专用紧凑时间：刚刚 / 12m / 7h / 3d / 09/07。 */
export function compactSessionTime(
  iso: string | null,
  justNowLabel: string,
  now = Date.now(),
): string {
  if (!iso) return "";
  const created = new Date(iso);
  const createdAt = created.getTime();
  if (!Number.isFinite(createdAt)) return "";

  const elapsed = Math.max(0, now - createdAt);
  const minutes = Math.floor(elapsed / MINUTE_MS);
  if (minutes < 1) return justNowLabel;
  if (minutes < 60) return `${minutes}m`;

  const hours = Math.floor(elapsed / HOUR_MS);
  if (hours < 24) return `${hours}h`;

  const days = Math.floor(elapsed / DAY_MS);
  if (days < 7) return `${days}d`;

  return `${pad2(created.getMonth() + 1)}/${pad2(created.getDate())}`;
}
