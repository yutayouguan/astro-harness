import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const welcomeSource = await readFile(
  new URL("../../components/chat/ChatWelcome.tsx", import.meta.url),
  "utf8",
);
const chatViewSource = await readFile(
  new URL("../../components/chat/ChatView.tsx", import.meta.url),
  "utf8",
);
const styles = await readFile(
  new URL("../../styles/features/chat/markdown.css", import.meta.url),
  "utf8",
);

test("welcome cards send localized templates with explicit slot hints", () => {
  assert.match(welcomeSource, /promptTemplateHints\(prompt\)/);
  assert.match(welcomeSource, /onPick\(prompt, promptTemplateHints\(prompt\)\)/);
});

test("composer renders, navigates, validates, and sanitizes welcome slots", () => {
  assert.match(chatViewSource, /welcomeTemplateActive/);
  assert.match(chatViewSource, /nextEmptyPromptTemplateSlot/);
  assert.match(chatViewSource, /preparePromptTemplateSend/);
  assert.match(chatViewSource, /chat\.welcomeTemplateNeedRequired/);
  assert.match(styles, /\.composer-input-wrap\.is-slot-template/);
});
