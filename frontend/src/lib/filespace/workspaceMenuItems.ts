/** 工作区右键菜单项生成。 */
import type { FileMenuAction } from "../../components/filespace/FileContextMenu";
import type { MessageKey } from "../../i18n/messages";

export type WorkspaceMenuItem = {
  action: FileMenuAction;
  labelKey: MessageKey;
  disabled?: boolean;
  danger?: boolean;
  separatorBefore?: boolean;
};

export function buildWorkspaceMenuItems(opts: {
  kind: "blank" | "entries";
  selectedCount: number;
  canPaste: boolean;
}): WorkspaceMenuItem[] {
  if (opts.kind === "blank") {
    return [
      { action: "newFile", labelKey: "workspace.menu.newFile" },
      { action: "newFolder", labelKey: "workspace.menu.newFolder" },
      {
        action: "paste",
        labelKey: "workspace.menu.paste",
        disabled: !opts.canPaste,
        separatorBefore: true,
      },
    ];
  }
  const multi = opts.selectedCount > 1;
  const none = opts.selectedCount < 1;
  return [
    { action: "open", labelKey: "workspace.menu.open", disabled: none },
    {
      action: "newFile",
      labelKey: "workspace.menu.newFile",
      separatorBefore: true,
    },
    { action: "newFolder", labelKey: "workspace.menu.newFolder" },
    {
      action: "cut",
      labelKey: "workspace.menu.cut",
      disabled: none,
      separatorBefore: true,
    },
    { action: "copyFile", labelKey: "workspace.menu.copy", disabled: none },
    {
      action: "paste",
      labelKey: "workspace.menu.paste",
      disabled: !opts.canPaste,
    },
    {
      action: "copyPath",
      labelKey: "workspace.menu.copyPath",
      disabled: none,
      separatorBefore: true,
    },
    {
      action: "rename",
      labelKey: "workspace.menu.rename",
      disabled: multi || none,
    },
    {
      action: "reveal",
      labelKey: "workspace.menu.reveal",
      disabled: multi || none,
    },
    {
      action: "openExternally",
      labelKey: "workspace.menu.openExternally",
      disabled: none,
    },
    {
      action: "trash",
      labelKey: "workspace.menu.trash",
      disabled: none,
      danger: true,
      separatorBefore: true,
    },
  ];
}
