import assert from "node:assert/strict";
import { test } from "node:test";
import { en, zh, type MessageKey } from "../../i18n/messages.ts";
import {
  listPromptTemplateSegments,
  nextEmptyPromptTemplateSlot,
  preparePromptTemplateSend,
  promptTemplateHints,
} from "./promptTemplate.ts";

const CARD_IDS = [
  "intro",
  "skills",
  "files",
  "data",
  "image",
  "music",
  "video",
  "web",
  "code",
  "writing",
  "search",
  "translate",
] as const;

test("every welcome card prompt exposes localized fillable slots", () => {
  for (const messages of [zh, en]) {
    for (const id of CARD_IDS) {
      const key = `chat.card.${id}.prompt` as MessageKey;
      const prompt = messages[key];
      const hints = promptTemplateHints(prompt);
      assert.ok(hints.length >= 2, `${key} should expose at least two slots`);
      assert.equal(
        preparePromptTemplateSend(prompt, hints).missing.length,
        hints.length,
      );
    }
  }
});

test("filled slots keep visual segments and serialize to plain prompt text", () => {
  const template = "请用「编程语言」实现「功能」。";
  const hints = promptTemplateHints(template);
  const partiallyFilled = template.replace("「编程语言」", "「Rust」");
  const segments = listPromptTemplateSegments(partiallyFilled, hints, [0, 1]);
  const slots = segments.filter((segment) => segment.type === "slot");

  assert.equal(slots.length, 2);
  assert.equal(slots[0]?.empty, false);
  assert.equal(slots[1]?.empty, true);
  assert.equal(
    nextEmptyPromptTemplateSlot(partiallyFilled, -1, hints)?.index,
    1,
  );

  const completed = partiallyFilled.replace("「功能」", "「命令行工具」");
  const prepared = preparePromptTemplateSend(completed, hints);
  assert.equal(prepared.ok, true);
  assert.equal(prepared.sanitized, "请用Rust实现命令行工具。");
});
