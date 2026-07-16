/** 创建 Agent 引导用的工作区模板文案。 */
// code/astro/frontend/src/lib/agentCreateTemplate.ts
export const AGENT_CREATE_TEMPLATE_ZH =
  "帮我创建一个助手：名称是「」，背景经历是「」，说话风格是「」，主要帮我做「」，不要做「」，请称呼我为「」，我的偏好是「」";

export const AGENT_CREATE_TEMPLATE_EN =
  "Help me create an assistant: name is 「」, background is 「」, speaking style is 「」, mainly help me with 「」, do not 「」, call me 「」, my preferences are 「」";

export type BracketSlot = {
  index: number;
  open: number;
  close: number;
  innerStart: number;
  innerEnd: number;
  empty: boolean;
};

const OPEN = "「";
const CLOSE = "」";

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
    slots.push({
      index,
      open,
      close,
      innerStart,
      innerEnd,
      empty: innerStart === innerEnd,
    });
    index += 1;
    i = close + CLOSE.length;
  }
  return slots;
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


/** 取模板中第一个「」槽位的当前文本，供创建引导预览 */
export function firstSlotValue(text: string): string {
  const slot = listSlots(text)[0];
  if (!slot) return "";
  return text.slice(slot.innerStart, slot.innerEnd).trim();
}
