/** 创建 Agent 引导用的工作区模板文案。 */

/** 中文槽位提示（填在「」内，便于识别与 Tab 跳转） */
export const SLOT_HINTS_ZH = [
  "名称",
  "背景",
  "风格",
  "做什么",
  "不要做",
  "称呼",
  "偏好",
] as const;

/** 英文槽位提示 */
export const SLOT_HINTS_EN = [
  "name",
  "background",
  "style",
  "help with",
  "avoid",
  "call me",
  "prefs",
] as const;

const SLOT_HINT_SET = new Set<string>([...SLOT_HINTS_ZH, ...SLOT_HINTS_EN]);

export const AGENT_CREATE_TEMPLATE_ZH =
  "帮我创建一个助手：名称是「名称」，背景经历是「背景」，说话风格是「风格」，主要帮我做「做什么」，不要做「不要做」，请称呼我为「称呼」，我的偏好是「偏好」";

export const AGENT_CREATE_TEMPLATE_EN =
  "Help me create an assistant: name is 「name」, background is 「background」, speaking style is 「style」, mainly help me with 「help with」, do not 「avoid」, call me 「call me」, my preferences are 「prefs」";

export type BracketSlot = {
  index: number;
  open: number;
  close: number;
  innerStart: number;
  innerEnd: number;
  empty: boolean;
};

export type TemplateSegment =
  | { type: "text"; value: string }
  | { type: "slot"; value: string; empty: boolean; open: string; close: string };

const OPEN = "「";
const CLOSE = "」";

/** 槽位内容是否仍为占位提示（未真正填写） */
export function isSlotHint(inner: string): boolean {
  return SLOT_HINT_SET.has(inner.trim());
}

export function listSlots(text: string): BracketSlot[] {
  const slots: BracketSlot[] = [];
  let i = 0;
  let index = 0;
  while (i < text.length) {
    const open = text.indexOf(OPEN, i);
    if (open < 0) break;
    const close = text.indexOf(CLOSE, open + OPEN.length);
    if (close < 0) break;
    const innerStart = open + OPEN.length;
    const innerEnd = close;
    const inner = text.slice(innerStart, innerEnd);
    slots.push({
      index,
      open,
      close,
      innerStart,
      innerEnd,
      empty: innerStart === innerEnd || isSlotHint(inner),
    });
    index += 1;
    i = close + CLOSE.length;
  }
  return slots;
}

/** 将模板拆成普通文本 + 可高亮槽位，供输入框镜像渲染 */
export function listTemplateSegments(text: string): TemplateSegment[] {
  const slots = listSlots(text);
  if (slots.length === 0) {
    return text ? [{ type: "text", value: text }] : [];
  }
  const out: TemplateSegment[] = [];
  let cursor = 0;
  for (const slot of slots) {
    if (slot.open > cursor) {
      out.push({ type: "text", value: text.slice(cursor, slot.open) });
    }
    out.push({
      type: "slot",
      value: text.slice(slot.innerStart, slot.innerEnd),
      empty: slot.empty,
      open: OPEN,
      close: CLOSE,
    });
    cursor = slot.close + CLOSE.length;
  }
  if (cursor < text.length) {
    out.push({ type: "text", value: text.slice(cursor) });
  }
  return out;
}

export function findSlotAt(
  text: string,
  caret: number,
): BracketSlot | null {
  for (const slot of listSlots(text)) {
    if (caret >= slot.open && caret <= slot.close + CLOSE.length) {
      return slot;
    }
  }
  return null;
}

export function nextEmptySlot(
  text: string,
  fromCaret: number,
): BracketSlot | null {
  const slots = listSlots(text).filter((s) => s.empty);
  return slots.find((s) => s.innerStart > fromCaret) ?? slots[0] ?? null;
}

export function prevEmptySlot(
  text: string,
  fromCaret: number,
): BracketSlot | null {
  const slots = listSlots(text).filter((s) => s.empty);
  for (let i = slots.length - 1; i >= 0; i--) {
    if (slots[i].innerEnd < fromCaret) return slots[i];
  }
  return slots[slots.length - 1] ?? null;
}

export function templateForLocale(locale: string): string {
  return locale.startsWith("en")
    ? AGENT_CREATE_TEMPLATE_EN
    : AGENT_CREATE_TEMPLATE_ZH;
}

/** 取模板中第一个「」槽位的当前文本，供创建引导预览；占位提示视为未填 */
export function firstSlotValue(text: string): string {
  const slot = listSlots(text)[0];
  if (!slot) return "";
  const value = text.slice(slot.innerStart, slot.innerEnd).trim();
  if (!value || isSlotHint(value)) return "";
  return value;
}
