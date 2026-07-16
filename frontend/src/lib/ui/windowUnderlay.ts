/** 原生窗 underlay 与主题底色同步。 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";

/** 与 base.css --window-underlay 对齐，填满系统圆角抗锯齿缝 */
const UNDERLAY: Record<"light" | "dark", Record<string, string>> = {
  light: {
    blue: "#dbeafe",
    green: "#dcfce7",
    purple: "#ede9fe",
    orange: "#ffedd5",
    pink: "#fce7f3",
    cyan: "#cffafe",
    indigo: "#e0e7ff",
    amber: "#fef3c7",
    teal: "#ccfbf1",
    default: "#e9eef6",
  },
  dark: {
    blue: "#0a1018",
    green: "#07140f",
    purple: "#100816",
    orange: "#16120c",
    pink: "#1a0c16",
    cyan: "#0a161a",
    indigo: "#0c0c1a",
    amber: "#16120c",
    teal: "#071412",
    default: "#100816",
  },
};

export function underlayColor(
  theme: "light" | "dark",
  tone: string,
): string {
  const map = UNDERLAY[theme];
  return map[tone] ?? map.default;
}

/** 同步原生窗 / WebView 底色，避免四角露缝 */
export async function syncWindowUnderlay(
  theme: "light" | "dark",
  tone: string,
): Promise<void> {
  const color = underlayColor(theme, tone);
  try {
    await Promise.all([
      getCurrentWindow().setBackgroundColor(color),
      getCurrentWebview().setBackgroundColor(color),
    ]);
  } catch {
    // 浏览器预览或权限未就绪时忽略
  }
}
