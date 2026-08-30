import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const component = await readFile(
  new URL("../../components/chat/ChatMarkdown.tsx", import.meta.url),
  "utf8",
);
const markdownStyles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);
const answerStyles = await readFile(
  new URL("../../styles/features/chat/answer-panel.css", import.meta.url),
  "utf8",
);
const coreStyles = await readFile(
  new URL("../../styles/features/chat/core.css", import.meta.url),
  "utf8",
);

function rule(css, selector) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(
    new RegExp(`(?:^|\\n)${escaped}\\s*\\{(?<body>[\\s\\S]*?)\\}`),
  )?.groups?.body;
}

test("markdown code blocks use a compact matte surface", () => {
  const block = rule(markdownStyles, ".msg-md-codeblock");
  const header = rule(markdownStyles, ".msg-md-code-header");
  const language = rule(markdownStyles, ".msg-md-code-lang");
  const pre = rule(markdownStyles, ".msg-md-pre");

  assert.ok(block, "missing shared code block surface");
  assert.match(block, /border-radius:\s*11px;/);
  assert.match(block, /box-shadow:\s*none;/);
  assert.match(block, /backdrop-filter:\s*none;/);
  assert.ok(header, "missing compact code header");
  assert.match(header, /min-height:\s*38px;/);
  assert.ok(language, "missing language label styles");
  assert.match(language, /font-size:\s*11px;/);
  assert.doesNotMatch(language, /text-transform:\s*uppercase;/);
  assert.ok(pre, "missing code body styles");
  assert.match(pre, /padding:\s*14px 16px 16px;/);
  assert.match(pre, /font-size:\s*13\.5px;/);
  assert.match(pre, /overflow-x:\s*auto;/);
});

test("copy feedback is visible, accessible, and self-clearing", () => {
  const button = rule(markdownStyles, ".msg-md-code-copy");
  const feedback = rule(markdownStyles, ".msg-md-code-copy-feedback");

  assert.match(component, /function CodeCopyButton\(/);
  assert.match(component, /role="status" aria-live="polite"/);
  assert.match(component, /window\.clearTimeout\(resetTimerRef\.current\)/);
  assert.match(component, /}, 1200\);/);
  assert.match(component, /data-single-line=\{!codeText\.includes\("\\n"\) \|\| undefined\}/);
  assert.ok(button, "missing copy button styles");
  assert.match(button, /width:\s*28px;/);
  assert.match(button, /opacity:\s*0\.72;/);
  assert.ok(feedback, "missing copied feedback label");
  assert.match(feedback, /pointer-events:\s*none;/);
});

test("answer panel no longer reapplies the heavy glass code surface", () => {
  const answerBlock = rule(answerStyles, ".bubble.assistant .msg-md-codeblock");

  assert.ok(answerBlock, "missing answer code block refinement");
  assert.match(answerBlock, /border-radius:\s*11px;/);
  assert.doesNotMatch(
    coreStyles,
    /\.bubble\.assistant \.msg-activity-detail,\n\.bubble\.assistant \.msg-md-codeblock,/,
  );
});
