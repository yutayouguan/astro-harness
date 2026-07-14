/** 将毫秒差格式化为展示用秒数（&lt;10s 一位小数） */
export function formatElapsedSec(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "0";
  if (sec < 10) return sec.toFixed(1);
  return String(Math.round(sec));
}

/** startedAt(ms) → 当前/截止 的秒数，至少 0.1 */
export function elapsedSecSince(startedAtMs: number, endedAtMs = Date.now()): number {
  return Math.max(0.1, Math.round(((endedAtMs - startedAtMs) / 1000) * 10) / 10);
}
