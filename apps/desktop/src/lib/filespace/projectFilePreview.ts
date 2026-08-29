import {
  filespaceViewerKind,
  type FilespaceViewerKind,
} from "./filespaceViewerKind.ts";

export type ProjectFilePreviewKind = Exclude<FilespaceViewerKind, "missing">;

export type ProjectFileOpenPlan = {
  kind: ProjectFilePreviewKind;
  readAsText: boolean;
  readonly: boolean;
};

/**
 * 项目文件打开前先按类型分流，避免将媒体/PDF 误作 UTF-8 文本读取。
 */
export function projectFileOpenPlan(name: string): ProjectFileOpenPlan {
  const resolved = filespaceViewerKind({ name });
  const kind = resolved === "missing" ? "external" : resolved;
  const readAsText = kind === "text" || kind === "markdown" || kind === "html";
  return { kind, readAsText, readonly: !readAsText };
}
