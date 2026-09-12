import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const read = (path) => readFile(new URL(path, import.meta.url), "utf8");
const [view, css] = await Promise.all([
  read("../../components/chat/ChatView.tsx"),
  read("../../styles/features/chat/markdown.css"),
]);

test("normal and queued send use a real upward arrow, while stop remains a square", () => {
  const actions = view.slice(
    view.indexOf('className="send-btn send-btn--round send-btn--stop"'),
  );
  assert.equal(
    actions.match(/<ArrowUp size=\{19\} strokeWidth=\{2.2\} aria-hidden \/>/g)
      ?.length,
    2,
  );
  assert.doesNotMatch(view, /SendHorizontal/);
  assert.match(actions, /<Square/);
  assert.doesNotMatch(
    css,
    /\.send-btn--round:not\(\.send-btn--stop\) svg\s*\{\s*transform: rotate/,
  );
});

test("themed send does not recolor the stop control or use a fixed blue fallback", () => {
  assert.match(
    css,
    /html\[data-theme="light"\] \.composer--stacked \.send-btn:not\(\.send-btn--stop\)\s*\{\s*background: var\(--tone, var\(--accent\)\)/,
  );
  assert.match(css, /\.send-btn:disabled\s*\{[^}]*opacity: 0\.4/);
});
