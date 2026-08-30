import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const root = new URL("../../", import.meta.url);

async function source(path) {
  return readFile(new URL(path, root), "utf8");
}

test("reasoning and tool activity share one quiet status-row contract", async () => {
  const [index, css] = await Promise.all([
    source("styles/index.css"),
    source("styles/features/chat/activity-surfaces.css"),
  ]);

  assert.match(index, /tool-activity-polish\.css[\s\S]*activity-surfaces\.css/);
  assert.match(css, /--activity-row-height: 36px/);
  assert.match(css, /\.msg-reasoning,[\s\S]*> \.msg-activity \{/);
  assert.match(css, /border-radius: var\(--activity-row-radius\)/);
  assert.match(
    css,
    /\.msg-reasoning-toggle,[\s\S]*\.msg-activity-toggle,[\s\S]*\.msg-activity-summary[\s\S]*min-height: var\(--activity-row-height\)/,
  );
  assert.doesNotMatch(css, /border-radius: 999px/);
});

test("grouped activity rows and reply icons share balanced alignment", async () => {
  const [activity, groups] = await Promise.all([
    source("styles/features/chat/activity.css"),
    source("styles/features/chat/activity-groups.css"),
  ]);

  assert.match(
    groups,
    /msg-activity-group-items \.msg-activity-body \{[\s\S]*gap: 0/,
  );
  assert.match(
    activity,
    /msg-timeline-step\.kind-reply \.msg-timeline-rail \{[\s\S]*padding-top: 7px/,
  );
});

test("expanded tool activity gains depth while running state avoids full-card glow", async () => {
  const css = await source("styles/features/chat/activity-surfaces.css");

  assert.match(css, /> \.msg-activity\.is-open \{/);
  assert.match(css, /\.msg-activity\.is-open[\s\S]*\.msg-activity-collapse-inner/);
  assert.match(css, /\.msg-reasoning\.is-active \{[\s\S]*animation: none/);
  assert.match(css, /prefers-reduced-motion: reduce/);
  assert.match(css, /prefers-reduced-transparency: reduce/);
});

test("TODO updates stay out of answers and use a compact centered composer status", async () => {
  const [activity, progress, chatView, css] = await Promise.all([
    source("components/chat/MsgActivity.tsx"),
    source("components/chat/TodoProgress.tsx"),
    source("components/chat/ChatView.tsx"),
    source("styles/features/chat/activity-surfaces.css"),
  ]);

  assert.match(activity, /const \[inputOpen, setInputOpen\] = useState\(false\)/);
  assert.match(activity, /className="msg-activity-io-disclosure"/);
  assert.match(activity, /msg-activity-input-collapse/);
  assert.match(progress, /className="todo-progress-current"/);
  assert.match(chatView, /if \(isTodoOnlyActivityMessage\(m\)\) return null/);
  assert.match(chatView, /if \(isTodoActivity\(act\)\) return/);
  assert.match(chatView, /<TodoProgress[\s\S]*messages=\{messages\}/);
  assert.match(
    css,
    /\.composer-shell \.todo-progress-float \{[\s\S]*align-self: center;[\s\S]*width: max-content;[\s\S]*max-width: min\(calc\(100% - 32px\), 520px\);[\s\S]*margin: 0 auto 6px/,
  );
  assert.match(
    css,
    /\.composer-shell \.todo-progress-popover \{[\s\S]*left: 50%;[\s\S]*transform: translateX\(-50%\);[\s\S]*transform-origin: center bottom/,
  );
});
