import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const cssUrl = new URL("../../styles/features/shell/header.css", import.meta.url);
const titleUrl = new URL("../../components/chat/ConversationTitle.tsx", import.meta.url);
const tipsUrl = new URL("../../hooks/ui/useBeautifyTips.ts", import.meta.url);

test("conversation title truncates and only exposes its full text when overflowing", async () => {
  const [css, source, tips] = await Promise.all([
    readFile(cssUrl, "utf8"),
    readFile(titleUrl, "utf8"),
    readFile(tipsUrl, "utf8"),
  ]);

  assert.match(css, /\.conversation-title[\s\S]*?text-overflow: ellipsis;/);
  assert.match(source, /scrollWidth > node\.clientWidth \+ 1/);
  assert.match(source, /data-tip=\{overflowing \? title : undefined\}/);
  assert.match(source, /data-tip-delay=\{overflowing \? "400" : undefined\}/);
  assert.match(source, /onClick=\{onRename\}/);
  assert.match(tips, /getAttribute\("data-tip-delay"\)/);
  assert.match(tips, /showFor\(el, true\)/);
});

test("header tool groups reuse the assistant answer surface recipe", async () => {
  const css = await readFile(cssUrl, "utf8");
  const expectedSurface = [
    "background: var(--glass-layer, var(--glass-panel));",
    "border: 0.5px solid var(--glass-edge);",
    "box-shadow: var(--shadow-card), var(--glass-rim);",
    "backdrop-filter: var(--backdrop-glass);",
  ];

  for (const selector of [".chat-header-tools", ".model-picker-trigger"]) {
    const blocks = [
      ...css.matchAll(new RegExp(`\\${selector} \\{([\\s\\S]*?)\\n\\}`, "g")),
    ].map((match) => match[1]);
    assert.ok(
      blocks.some((block) => expectedSurface.every((declaration) => block.includes(declaration))),
    );
  }
});
