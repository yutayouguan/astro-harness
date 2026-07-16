/**
 * 将秒数格式化为带单位的展示文案。
 * `<1s` → `12ms`；`<10s` → `1.2s`；否则整秒。
 */
export function formatElapsedSec(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "0ms";
  const ms = Math.round(sec * 1000);
  if (ms < 1000) return `${ms}ms`;
  const s = ms / 1000;
  if (s < 10) return `${s.toFixed(1)}s`;
  return `${Math.round(s)}s`;
}

/** startedAt(ms) → 秒数，精度到毫秒，无人为下限 */
export function elapsedSecSince(startedAtMs: number, endedAtMs = Date.now()): number {
  const ms = Math.max(0, endedAtMs - startedAtMs);
  return Math.round(ms) / 1000;
}
