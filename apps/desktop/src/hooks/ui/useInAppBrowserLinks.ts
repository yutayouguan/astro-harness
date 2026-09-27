/**
 * 把界面里的网页链接默认交给内置浏览器。
 *
 * 文档级监听（冒泡阶段），只接管普通左键点击的 http(s) 链接；`enabled=false`
 * 时完全不介入（例如内置浏览器坞不可用的页面，保持 WebView / 系统浏览器行为）。
 */
import { useEffect, useRef } from "react";
import { resolveInAppBrowserLink } from "../../lib/browser/inAppBrowserLink";

export function useInAppBrowserLinks({
  enabled,
  onOpen,
}: {
  enabled: boolean;
  onOpen: (url: string) => void;
}): void {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;
  const onOpenRef = useRef(onOpen);
  onOpenRef.current = onOpen;

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const url = resolveInAppBrowserLink(event, {
        dockAvailable: enabledRef.current,
      });
      if (!url) return;
      // 接管后不能再让 WebView 打开新窗口 / 系统浏览器。
      event.preventDefault();
      onOpenRef.current(url);
    };
    document.addEventListener("click", onClick);
    return () => document.removeEventListener("click", onClick);
  }, []);
}
