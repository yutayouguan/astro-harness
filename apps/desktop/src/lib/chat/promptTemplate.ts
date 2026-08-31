const SLOT_OPEN = "「";
const SLOT_CLOSE = "」";

export type PromptTemplateSlot = {
  index: number;
  open: number;
  close: number;
  innerStart: number;
  innerEnd: number;
  empty: boolean;
};

export type PromptTemplateSegment =
  | { type: "text"; value: string }
  | {
      type: "slot";
      value: string;
      empty: boolean;
      open: string;
      close: string;
      index: number;
      required: boolean;
    };

function hintSet(hints: readonly string[]): Set<string> {
  return new Set(hints.map((hint) => hint.trim()).filter(Boolean));
}

/** Extract the initial labels inside a localized prompt template. */
export function promptTemplateHints(template: string): string[] {
  return listPromptTemplateSlots(template).map((slot) =>
    template.slice(slot.innerStart, slot.innerEnd).trim(),
  );
}

export function listPromptTemplateSlots(
  text: string,
  hints: readonly string[] = [],
): PromptTemplateSlot[] {
  const slots: PromptTemplateSlot[] = [];
  const knownHints = hintSet(hints);
  let cursor = 0;
  let index = 0;
  while (cursor < text.length) {
    const open = text.indexOf(SLOT_OPEN, cursor);
    if (open < 0) break;
    const close = text.indexOf(SLOT_CLOSE, open + SLOT_OPEN.length);
    if (close < 0) break;
    const innerStart = open + SLOT_OPEN.length;
    const innerEnd = close;
    const inner = text.slice(innerStart, innerEnd).trim();
    slots.push({
      index,
      open,
      close,
      innerStart,
      innerEnd,
      empty: inner.length === 0 || knownHints.has(inner),
    });
    index += 1;
    cursor = close + SLOT_CLOSE.length;
  }
  return slots;
}

export function listPromptTemplateSegments(
  text: string,
  hints: readonly string[] = [],
  requiredIndices: readonly number[] = [],
): PromptTemplateSegment[] {
  const slots = listPromptTemplateSlots(text, hints);
  if (slots.length === 0) {
    return text ? [{ type: "text", value: text }] : [];
  }

  const required = new Set(requiredIndices);
  const segments: PromptTemplateSegment[] = [];
  let cursor = 0;
  for (const slot of slots) {
    if (slot.open > cursor) {
      segments.push({ type: "text", value: text.slice(cursor, slot.open) });
    }
    segments.push({
      type: "slot",
      value: text.slice(slot.innerStart, slot.innerEnd),
      empty: slot.empty,
      open: SLOT_OPEN,
      close: SLOT_CLOSE,
      index: slot.index,
      required: required.has(slot.index),
    });
    cursor = slot.close + SLOT_CLOSE.length;
  }
  if (cursor < text.length) {
    segments.push({ type: "text", value: text.slice(cursor) });
  }
  return segments;
}

export function findPromptTemplateSlotAt(
  text: string,
  caret: number,
  hints: readonly string[] = [],
): PromptTemplateSlot | null {
  return (
    listPromptTemplateSlots(text, hints).find(
      (slot) => caret >= slot.open && caret <= slot.close + SLOT_CLOSE.length,
    ) ?? null
  );
}

export function nextEmptyPromptTemplateSlot(
  text: string,
  fromCaret: number,
  hints: readonly string[] = [],
): PromptTemplateSlot | null {
  const slots = listPromptTemplateSlots(text, hints).filter((slot) => slot.empty);
  return slots.find((slot) => slot.innerStart > fromCaret) ?? slots[0] ?? null;
}

export function prevEmptyPromptTemplateSlot(
  text: string,
  fromCaret: number,
  hints: readonly string[] = [],
): PromptTemplateSlot | null {
  const slots = listPromptTemplateSlots(text, hints).filter((slot) => slot.empty);
  for (let index = slots.length - 1; index >= 0; index -= 1) {
    if (slots[index].innerEnd < fromCaret) return slots[index];
  }
  return slots[slots.length - 1] ?? null;
}

export function preparePromptTemplateSend(
  text: string,
  hints: readonly string[],
): { ok: boolean; missing: PromptTemplateSlot[]; sanitized: string } {
  const missing = listPromptTemplateSlots(text, hints).filter((slot) => slot.empty);
  return {
    ok: missing.length === 0,
    missing,
    sanitized: text.split(SLOT_OPEN).join("").split(SLOT_CLOSE).join("").trim(),
  };
}
