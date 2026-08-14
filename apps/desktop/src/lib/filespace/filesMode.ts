/** 「文件」页子模式：浏览工作区目录 / 浏览对话产物，持久化到 localStorage。 */

export type FilesSubmode = "browse" | "artifacts";

export const FILES_SUBMODE_KEY = "astro.files.submode";

export function readFilesSubmode(): FilesSubmode {
  try {
    const v = localStorage.getItem(FILES_SUBMODE_KEY);
    if (v === "browse" || v === "artifacts") return v;
  } catch {
    // ignore
  }
  return "browse";
}

export function writeFilesSubmode(mode: FilesSubmode): void {
  try {
    localStorage.setItem(FILES_SUBMODE_KEY, mode);
  } catch {
    // ignore
  }
}
