/** 按 VSCode 的匹配顺序解析 material-icon-theme 图标：文件名 → 复合扩展名 → 扩展名。 */
import { materialIconManifest } from "./generated/materialIconManifest.ts";

const ICON_BASE = "/file-icons/";

function iconUrl(icon: string): string {
  return `${ICON_BASE}${icon}.svg`;
}

export function materialFolderIconUrl(name: string, expanded = false): string {
  const base = (name.split(/[/\\]/).pop() ?? name).toLowerCase();
  const named = expanded
    ? materialIconManifest.folderNamesExpanded[base]
    : materialIconManifest.folderNames[base];
  if (named) return iconUrl(named);
  return iconUrl(
    expanded
      ? materialIconManifest.defaults.folderExpanded
      : materialIconManifest.defaults.folder,
  );
}

export function materialFileIconUrl(name: string): string {
  const base = (name.split(/[/\\]/).pop() ?? name).toLowerCase();
  const exact = materialIconManifest.fileNames[base];
  if (exact) return iconUrl(exact);

  // `foo.spec.ts` 先试 `spec.ts` 再试 `ts`，与 VSCode 的后缀匹配一致
  for (let dot = base.indexOf("."); dot !== -1; dot = base.indexOf(".", dot + 1)) {
    const byExtension = materialIconManifest.fileExtensions[base.slice(dot + 1)];
    if (byExtension) return iconUrl(byExtension);
  }

  return iconUrl(materialIconManifest.defaults.file);
}

export function materialIconUrl(
  name: string,
  { isDir = false, expanded = false }: { isDir?: boolean; expanded?: boolean } = {},
): string {
  return isDir ? materialFolderIconUrl(name, expanded) : materialFileIconUrl(name);
}
