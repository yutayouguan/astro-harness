import assert from "node:assert/strict";
import test from "node:test";
import {
  assistantAnswerPlainText,
  assistantProcessMarkdown,
} from "./assistantMessageClipboard.ts";

test("converts an answer to readable plain text", () => {
  assert.equal(
    assistantAnswerPlainText(
      "## 结果\n\n- **完成** [文档](https://example.com)\n- `npm test`",
    ),
    "结果\n\n完成 文档\nnpm test",
  );
});

test("preserves literal underscores and home-relative paths", () => {
  assert.equal(
    assistantAnswerPlainText(
      "Run `fetch_title` in `~/generated_code` and keep snake_case.",
    ),
    "Run fetch_title in ~/generated_code and keep snake_case.",
  );
});

test("serializes reasoning, tools, and answer in categorized order", () => {
  const result = assistantProcessMarkdown({
    id: "a1",
    role: "assistant",
    reasoning: "先检查。",
    content: "**已完成**",
    activities: [
      {
        id: "tool-1",
        kind: "tool",
        title: "file_ops",
        input: '{"path":"README.md"}',
        output: "README content",
      },
    ],
  });

  assert.match(result, /^## 思考/);
  assert.match(result, /## 工具\n\n### file_ops/);
  assert.match(result, /## 回答\n\n\*\*已完成\*\*$/);
});
