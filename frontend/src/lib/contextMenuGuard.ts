/** 默认可弹出右键菜单的区域（与 user-select 白名单对齐）。 */
const ALLOW_SELECTOR = [
  "input",
  "textarea",
  "select",
  '[contenteditable="true"]',
  ".user-select-text",
  ".msg-content",
  ".msg-md",
  ".msg-activity-body",
  "pre",
  "code",
  ".cm-editor",
  ".cm-content",
  ".ws-editor",
  ".fs-preview-text",
  ".chat-agent-preview",
  ".skills-preview",
  ".skills-preview-backdrop",
  "[data-allow-context-menu]",
].join(", ");

/**
 * 在无业务用途的区域拦截浏览器默认右键菜单。
 * 文件树等自定义菜单仍可在 bubble 阶段自行 `preventDefault` 并展示。
 */
export function installContextMenuGuard(): () => void {
  const onContextMenu = (e: MouseEvent) => {
    const el = e.target;
    if (!(el instanceof Element)) {
      e.preventDefault();
      return;
    }
    if (el.closest(ALLOW_SELECTOR)) return;
    e.preventDefault();
  };
  document.addEventListener("contextmenu", onContextMenu, true);
  return () => document.removeEventListener("contextmenu", onContextMenu, true);
}
