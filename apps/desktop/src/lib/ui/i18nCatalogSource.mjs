import { readFileSync } from "node:fs";
import { readFile } from "node:fs/promises";

/**
 * 字典源码面：中英已从 i18n/messages.ts 拆到 catalogs/{zh,en}.ts
 * （英文按需加载）。断言文案/键的用例读这两个文件；
 * messages.ts 只保留入口与 translate。
 */
export const i18nZhUrl = new URL("../../i18n/catalogs/zh.ts", import.meta.url);
export const i18nEnUrl = new URL("../../i18n/catalogs/en.ts", import.meta.url);

export async function readI18nCatalogs() {
  const [zh, en] = await Promise.all([
    readFile(i18nZhUrl, "utf8"),
    readFile(i18nEnUrl, "utf8"),
  ]);
  return `${zh}\n${en}`;
}

export function readI18nCatalogsSync() {
  return `${readFileSync(i18nZhUrl, "utf8")}\n${readFileSync(i18nEnUrl, "utf8")}`;
}
