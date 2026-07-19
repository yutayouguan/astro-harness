// code/astro/frontend/src/lib/agentCreateTemplate.test.ts
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
  AGENT_CREATE_TEMPLATE_ZH,
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
