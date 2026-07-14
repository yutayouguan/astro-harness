/** 强制让 WKWebView 跟窗体内尺寸对齐（macOS 放大/动画易不同步）。 */
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";

let syncing = false;

/** 将 webview 尺寸设为当前窗体内径；失败时静默忽略。 */
export async function syncWebviewToWindow(): Promise<void> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return;
  }
  if (syncing) return;
  syncing = true;
  try {
    const win = getCurrentWindow();
    const webview = getCurrentWebview();
    // innerSize 为物理像素，与 Webview.setSize 一致
    const size = await win.innerSize();
    await webview.setSize(size);
  } catch {
    // 权限未就绪 / 非 Tauri 环境
  } finally {
    syncing = false;
  }
}

/** 连拍两帧再同步，给 AppKit 布局落地时间。 */
export function syncWebviewToWindowSoon(): void {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      void syncWebviewToWindow();
    });
  });
}
