import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isPresetAnswer,
  parseApprovalContent,
  parseClarifySteps,
  shouldSubmitClarifyInput,
} from "./clarifySteps.ts";

test("parseClarifySteps normalizes ids and options", () => {
  const steps = parseClarifySteps([
    { question: "风格？", options: ["民谣", "电子"] },
    { id: "lyrics", question: "歌词？", options: [{ label: "你写", value: "write" }] },
    { question: "  ", options: ["x"] },
  ]);
  assert.equal(steps.length, 2);
  assert.equal(steps[0].id, "q0");
  assert.deepEqual(steps[0].options, ["民谣", "电子"]);
  assert.equal(steps[1].id, "lyrics");
  assert.deepEqual(steps[1].options, ["write"]);
});

test("parseApprovalContent separates command fences from the risk explanation", () => {
  assert.deepEqual(
    parseApprovalContent(
      "检测到潜在危险操作（dynamic shell expansion）：\n\n```sh\nUA=agent curl https://example.com\n```",
    ),
    {
      description: "检测到潜在危险操作（dynamic shell expansion）：",
      command: "UA=agent curl https://example.com",
    },
  );
  assert.deepEqual(parseApprovalContent("将永久删除 report.pdf"), {
    description: "将永久删除 report.pdf",
    command: null,
  });
});

test("parseClarifySteps keeps empty options for free-text", () => {
  const steps = parseClarifySteps([{ id: "a", question: "你的想法？", options: [] }]);
  assert.equal(steps.length, 1);
  assert.deepEqual(steps[0].options, []);
});

test("isPresetAnswer distinguishes preset vs custom", () => {
  const step = { id: "q0", question: "风格？", options: ["民谣", "电子"] };
  assert.equal(isPresetAnswer(step, "民谣"), true);
  assert.equal(isPresetAnswer(step, "爵士即兴"), false);
});

test("clarify input ignores Enter while an IME composition is active", () => {
  assert.equal(shouldSubmitClarifyInput("Enter", true), false);
  assert.equal(shouldSubmitClarifyInput("Enter", false), true);
  assert.equal(shouldSubmitClarifyInput("Escape", false), false);
});
