import type { Locale } from "../../i18n/messages";

function localeTag(locale: Locale): string {
  return locale === "zh" ? "zh-CN" : "en-US";
}

/** 按界面语言压缩大数：中文使用“万”，英文使用 K/M。 */
export function formatCompactTurnTokens(n: number, locale: Locale): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  return new Intl.NumberFormat(localeTag(locale), {
    notation: "compact",
    compactDisplay: "short",
    maximumFractionDigits: 1,
  }).format(Math.round(n));
}

/** 无缩写的本地化数字，用于 aria 和精确提示。 */
export function formatExactTurnTokens(n: number, locale: Locale): string {
  if (!Number.isFinite(n) || n <= 0) return "0";
  return new Intl.NumberFormat(localeTag(locale), {
    maximumFractionDigits: 0,
  }).format(Math.round(n));
}

/** 将较长的回合耗时转成易读的分钟 + 秒。 */
export function formatTurnDuration(sec: number, locale: Locale): string {
  if (!Number.isFinite(sec) || sec < 0) return locale === "zh" ? "0秒" : "0s";
  if (sec < 60) {
    const rounded = sec < 10 ? Math.round(sec * 10) / 10 : Math.round(sec);
    const value = Number.isInteger(rounded)
      ? String(rounded)
      : rounded.toFixed(1);
    return locale === "zh" ? `${value}秒` : `${value}s`;
  }

  const totalSeconds = Math.round(sec);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (locale === "zh") {
    return seconds > 0 ? `${minutes}分${seconds}秒` : `${minutes}分钟`;
  }
  return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
}
