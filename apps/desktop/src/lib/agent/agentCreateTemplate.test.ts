// apps/desktop/src/lib/agentCreateTemplate.test.ts
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  findSlotAt,
  firstSlotValue,
  nextEmptySlot,
  prevEmptySlot,
  listSlots,
  listTemplateSegments,
  isSlotHint,
  prepareAgentCreateSend,
  AGENT_CREATE_TEMPLATE_ZH,
  AGENT_CREATE_TEMPLATE_EN,
} from "./agentCreateTemplate.ts";

const SAMPLE =
  "帮我创建一个助手：名称是「」，背景经历是「已填」，说话风格是「」";

test("listSlots finds all bracket pairs", () => {
  const slots = listSlots(SAMPLE);
  assert.equal(slots.length, 3);
  assert.equal(SAMPLE.slice(slots[0].innerStart, slots[0].innerEnd), "");
  assert.equal(SAMPLE.slice(slots[1].innerStart, slots[1].innerEnd), "已填");
  assert.equal(slots[0].empty, true);
  assert.equal(slots[1].empty, false);
});

test("hint labels count as empty slots", () => {
  const slots = listSlots(AGENT_CREATE_TEMPLATE_ZH);
  assert.equal(slots.length, 7);
  assert.ok(slots.every((s) => s.empty));
  assert.equal(isSlotHint("名称"), true);
  assert.equal(firstSlotValue(AGENT_CREATE_TEMPLATE_ZH), "");
});

test("listTemplateSegments marks empty slots", () => {
  const segs = listTemplateSegments(AGENT_CREATE_TEMPLATE_ZH);
  const slots = segs.filter((s) => s.type === "slot");
  assert.equal(slots.length, 7);
  assert.ok(slots.every((s) => s.type === "slot" && s.empty));
  assert.ok(slots[0].type === "slot" && slots[0].required);
  assert.ok(slots[3].type === "slot" && slots[3].required);
  assert.ok(slots[1].type === "slot" && !slots[1].required);
});

test("findSlotAt returns slot when caret inside brackets", () => {
  const slots = listSlots(SAMPLE);
  const hit = findSlotAt(SAMPLE, slots[0].innerStart);
  assert.ok(hit);
  assert.equal(hit.index, 0);
});

test("nextEmptySlot skips filled slots", () => {
  const from = listSlots(SAMPLE)[0].innerEnd;
  const next = nextEmptySlot(SAMPLE, from);
  assert.ok(next);
  assert.equal(next.index, 2);
});

test("prevEmptySlot finds previous empty", () => {
  const from = listSlots(SAMPLE)[2].innerStart;
  const prev = prevEmptySlot(SAMPLE, from);
  assert.ok(prev);
  assert.equal(prev.index, 0);
});

test("prepareAgentCreateSend blocks when required slots empty", () => {
  const prep = prepareAgentCreateSend(AGENT_CREATE_TEMPLATE_ZH);
  assert.equal(prep.ok, false);
  assert.deepEqual(
    prep.missingRequired.map((s) => s.index),
    [0, 3],
  );
});

test("prepareAgentCreateSend strips optional empty clauses", () => {
  const filled =
    "帮我创建一个助手：名称是「小助」，背景经历是「背景」，说话风格是「风格」，主要帮我做「写代码」，不要做「不要做」，请称呼我为「称呼」，我的偏好是「偏好」";
  const prep = prepareAgentCreateSend(filled);
  assert.equal(prep.ok, true);
  assert.equal(prep.missingRequired.length, 0);
  assert.match(prep.sanitized, /名称是「小助」/);
  assert.match(prep.sanitized, /主要帮我做「写代码」/);
  assert.doesNotMatch(prep.sanitized, /背景经历/);
  assert.doesNotMatch(prep.sanitized, /说话风格/);
  assert.doesNotMatch(prep.sanitized, /不要做/);
  assert.doesNotMatch(prep.sanitized, /称呼我为/);
  assert.doesNotMatch(prep.sanitized, /偏好是/);
});

test("prepareAgentCreateSend keeps filled optional slots", () => {
  const filled =
    "帮我创建一个助手：名称是「小助」，背景经历是「十年产品」，说话风格是「风格」，主要帮我做「写代码」，不要做「不要做」，请称呼我为「称呼」，我的偏好是「偏好」";
  const prep = prepareAgentCreateSend(filled);
  assert.equal(prep.ok, true);
  assert.match(prep.sanitized, /背景经历是「十年产品」/);
  assert.doesNotMatch(prep.sanitized, /说话风格/);
});

test("prepareAgentCreateSend works for English template", () => {
  const filled = AGENT_CREATE_TEMPLATE_EN.replace("「name」", "「Astro」").replace(
    "「help with」",
    "「coding」",
  );
  const prep = prepareAgentCreateSend(filled);
  assert.equal(prep.ok, true);
  assert.match(prep.sanitized, /name is 「Astro」/);
  assert.match(prep.sanitized, /mainly help me with 「coding」/);
  assert.doesNotMatch(prep.sanitized, /background is/);
});
