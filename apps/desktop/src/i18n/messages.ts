/** 字典入口：中文常驻，英文按需加载；`translate` 只管插值。 */
import { zh, type MessageKey } from "./catalogs/zh.ts";

export type { MessageKey };
export type Locale = "zh" | "en";
export { zh };

export type MessageCatalog = Record<MessageKey, string>;

/** 当前语言字典；非当前语言只在切换时加载。 */
export async function loadLocaleMessages(
  locale: Locale,
): Promise<MessageCatalog> {
  if (locale === "zh") return zh;
  const { en } = await import("./catalogs/en.ts");
  return en;
}

export function translate(
  catalog: MessageCatalog,
  key: MessageKey,
  vars?: Record<string, string>,
): string {
  let text = catalog[key] ?? zh[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      text = text.split(`{${k}}`).join(v);
    }
  }
  return text;
}
