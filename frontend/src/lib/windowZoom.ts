/** 窗口缩放 / 还原：双击走与绿灯相同的原生 maximize（已关动画）。 */
import { getCurrentWindow } from "@tauri-apps/api/window";

/**
 * 双击标题栏：toggle maximize。
 * 不用自定义 setSize 贴工作区——在透明 Overlay 窗上会留下 WKWebView 白边；
 * 绿灯路径的 maximize 经实测与 WebView 同步正常。
 */
export async function zoomOrRestore() {
  const win = getCurrentWindow();
  document.documentElement.classList.add("zooming");
  try {
    await win.toggleMaximize();
  } finally {
    window.setTimeout(() => {
      document.documentElement.classList.remove("zooming");
    }, 40);
  }
}
